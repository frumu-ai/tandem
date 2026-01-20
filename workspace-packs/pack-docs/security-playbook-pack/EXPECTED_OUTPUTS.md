# Expected Outputs - Quality Criteria

This document defines what a successful run of the Security Playbook Pack produces and how to validate quality.

---

## Output Files Checklist

### Required Outputs

| File                             | Status | Quality Check                            |
| -------------------------------- | ------ | ---------------------------------------- |
| `outputs/security_context.md`    | ☐      | Complete context analysis                |
| `outputs/threat_assessment.md`   | ☐      | Risk register with prioritization        |
| `outputs/security_checklist.md`  | ☐      | Prioritized controls with implementation |
| `outputs/team_runbook.md`        | ☐      | Team-specific procedures                 |
| `outputs/security_playbook.html` | ☐      | Valid HTML, all sections present         |

---

## Quality Criteria by Output

### security_context.md

- [ ] Organizational baseline complete (size, industry, maturity)
- [ ] Threat environment overview with specific actors
- [ ] Compliance obligations summary by regulation
- [ ] Security posture assessment (strengths and gaps)
- [ ] Risk landscape overview with priorities
- [ ] All 4 input documents referenced and synthesized

**Red Flags**:

- Missing any of the 4 input document areas
- No specific threat actors identified
- Compliance gaps not severity-ranked
- Vague or generic descriptions

---

### threat_assessment.md

#### Risk Register Validation

- [ ] All 5 risk categories covered (Access, Data, Infrastructure, Human, Compliance)
- [ ] Each risk has likelihood and impact scores (1-5)
- [ ] Risk scores calculated correctly
- [ ] Current mitigations documented
- [ ] Residual risk assessed
- [ ] Treatment options assigned (Accept/Mitigate/Transfer/Avoid)
- [ ] Priority assigned (Critical/High/Medium/Low)

#### Priority Matrix Validation

- [ ] Quick wins identified (High impact, Low effort)
- [ ] Strategic investments identified (High impact, High effort)
- [ ] Recommendations are actionable
- [ ] Resource requirements acknowledged

**Red Flags**:

- Fewer than 10 risks documented
- Missing likelihood/impact justification
- No priority treatment recommendations
- Generic risks without specific context

---

### security_checklist.md

#### Control Documentation Validation

- [ ] At least 8 critical controls documented
- [ ] Each control has unique ID (SC-001 format)
- [ ] NIST CSF mapping included
- [ ] CIS Control referenced
- [ ] Implementation steps are actionable
- [ ] Owner assigned for each control
- [ ] Timeline specified
- [ ] Effort and cost estimated
- [ ] Priority assigned

#### Coverage Validation

- [ ] Multi-Factor Authentication covered
- [ ] Endpoint Detection covered
- [ ] Access Review Process covered
- [ ] Security Awareness Training covered
- [ ] Incident Response Plan covered
- [ ] Vulnerability Management covered
- [ ] Logging and Monitoring covered
- [ ] Data Backup covered

#### Timeline Validation

- [ ] Phase 1 (0-30 days) has specific controls
- [ ] Phase 2 (30-90 days) has specific controls
- [ ] Phase 3 (90-180 days) has specific controls
- [ ] Timeline is realistic given team constraints

**Red Flags**:

- Missing critical controls
- Implementation steps too vague to action
- No owner assigned
- Timeline ignores resource constraints

---

### team_runbook.md

#### Role Definition Validation

- [ ] Each team member has defined security role
- [ ] Responsibilities during normal ops defined
- [ ] Responsibilities during incidents defined
- Escalation paths documented
- Decision authority clear

#### Procedure Validation

- [ ] Security Incident Recognition procedure present
- [ ] New Employee Onboarding procedure present
- [ ] Employee Offboarding procedure present
- [ ] Phishing Response procedure present
- [ ] Vulnerability Response procedure present
- [ ] Each procedure has clear steps
- [ ] Response timeframes specified

#### Task Validation

- [ ] Daily security tasks (5 min) listed
- [ ] Weekly security tasks (30 min) listed
- [ ] Monthly security tasks (2 hours) listed
- [ ] Tasks are realistic for time allocation

#### Contact Information

- [ ] Security contacts list complete
- [ ] When to contact each person specified
- [ ] Emergency escalation path present

**Red Flags**:

- Procedures too generic
- Team members not referenced by name/role
- Missing key procedures
- Tasks exceed time allocation

---

### security_playbook.html

#### Technical Validation

- [ ] File opens in browser without errors
- [ ] All CSS loads correctly
- [ ] No broken links or missing resources
- [ ] Responsive design works on mobile
- [ ] All sections visible and readable
- [ ] Print stylesheet functional

#### Content Validation

- [ ] Playbook header complete
- [ ] Executive summary accurate
- [ ] Threat overview present
- [ ] Compliance status dashboard present
- [ ] Security controls matrix present
- [ ] Implementation roadmap present
- [ ] Team responsibilities section present
- [ ] Runbook quick links present
- [ ] Metrics section present
- [ ] Resources section present
- [ ] Document information section present

#### Design Validation

- [ ] Clean, professional appearance
- [ ] Good visual hierarchy
- [ ] Appropriate use of color
- [ ] Readable typography
- [ ] Consistent spacing and alignment
- [ ] Card-based layout functional

**Red Flags**:

- Doesn't open in browser
- Missing sections
- Broken layout
- Content doesn't match source documents
- Design is cluttered or hard to read

---

## Compliance Mapping Validation

Verify security checklist maps to requirements:

- [ ] SOC 2 requirements addressed
- [ ] Insurance requirements addressed
- [ ] Regulatory requirements addressed
- [ ] Customer contractual requirements addressed

---

## Resource Validation

Verify resource requirements are realistic:

- [ ] Timeline accounts for team constraints
- [ ] Budget estimates are reasonable
- [ ] Effort estimates are realistic
- [ ] External dependencies acknowledged

---

## Common Issues & Fixes

| Issue                | Likely Cause                | Fix                                         |
| -------------------- | --------------------------- | ------------------------------------------- |
| Incomplete context   | Didn't read all input files | Re-run Prompt 1 with full review            |
| Generic risks        | Surface-level analysis      | Re-run Prompt 2 with specific threats       |
| Unrealistic timeline | Ignored team constraints    | Re-run Prompt 3 with resource awareness     |
| Generic procedures   | Didn't use team names       | Re-run Prompt 4 with team profile reference |
| HTML broken          | Missing closing tags        | Re-run Prompt 5                             |

---

## Approvals Checklist

Before considering the pack run complete:

- [ ] Read approvals granted for all input files (4 files)
- [ ] Write approval granted for security_context.md
- [ ] Write approval granted for threat_assessment.md
- [ ] Write approval granted for security_checklist.md
- [ ] Write approval granted for team_runbook.md
- [ ] Write approval granted for security_playbook.html
- [ ] All outputs saved to correct paths
- [ ] HTML dashboard opens successfully
- [ ] All critical controls documented
- [ ] Team procedures are actionable
- [ ] Quality criteria met
