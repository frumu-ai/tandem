# Tandem Prompts - Security Playbook Pack

Copy and paste these prompts sequentially into Tandem. Each prompt builds on the previous outputs.

---

## Prompt 1: Security Context Analysis

```
You are a security architect building a comprehensive security playbook. Your task is to analyze the organizational context and create a security context summary.

Read ALL files in inputs/ including:
- inputs/company_context.md
- inputs/team_profile.md
- inputs/threat_landscape.md
- inputs/compliance_requirements.md

Create a security context summary document (outputs/security_context.md) that includes:

## Organizational Baseline
- Company size, industry, and risk profile
- Current security maturity level (use: Initial, Repeatable, Defined, Managed, Optimizing)
- Key business drivers and constraints
- Technical infrastructure overview

## Threat Environment Overview
- Relevant threat actors and motivations
- Attack vectors most likely to target this organization
- Industry-specific threats and trends
- Threat likelihood and impact summary

## Compliance Obligations Summary
- Key regulations and standards applicable
- Critical compliance gaps by severity
- Upcoming deadlines and requirements
- Audit cycles and documentation needs

## Security Posture Assessment
- Current strengths to leverage
- Critical gaps requiring immediate attention
- Resource constraints affecting security
- Team capabilities and limitations

## Risk Landscape Overview
- High-priority risk areas
- Risk tolerance indicators
- Key risk transfer considerations
- Recommended risk treatment approach

Save your summary to outputs/security_context.md and explain your analysis approach before writing.
```

---

## Prompt 2: Threat Assessment & Prioritization

```
You are a security risk analyst. Using the context summary and source documents, create a comprehensive threat assessment with prioritized risks.

## Threat Assessment Requirements

### Risk Register

For each significant risk, document:

#### Risk: [Name]
- **Description**: What the risk is
- **Threat Actors**: Who would exploit this
- **Attack Vector**: How it could be exploited
- **Likelihood**: 1-5 scale with justification
- **Impact**: 1-5 scale with justification
- **Risk Score**: Likelihood x Impact
- **Current Mitigations**: What's already in place
- **Residual Risk**: What's left after mitigations
- **Treatment Option**: Accept, Mitigate, Transfer, Avoid
- **Priority**: Critical/High/Medium/Low

### Risk Categories to Assess

1. **Access & Authentication Risks**
   - MFA gaps
   - Password policy weaknesses
   - Access review deficiencies
   - Remote access vulnerabilities

2. **Data Protection Risks**
   - Data handling gaps
   - Encryption gaps
   - Data retention issues
   - Third-party data risks

3. **Infrastructure Risks**
   - Cloud configuration issues
   - Vulnerability management gaps
   - Monitoring deficiencies
   - Backup/recovery concerns

4. **Human Risks**
   - Phishing susceptibility
   - Security awareness gaps
   - Insider threat potential
   - Training deficiencies

5. **Compliance Risks**
   - Policy documentation gaps
   - Audit readiness issues
   - Regulatory exposure
   - Contractual obligations

### Prioritization Framework

Create a priority matrix showing:
- Quick wins (High impact, Low effort)
- Strategic investments (High impact, High effort)
- Quick fixes (Low impact, Low effort)
- Defer/low priority (Low impact, High effort)

### Recommended Treatment Plan

For each priority area:
- Specific actions to take
- Owner assignment
- Timeline expectations
- Resource requirements

Save to outputs/threat_assessment.md
```

---

## Prompt 3: Priority Security Checklist

```
You are a security program manager creating an actionable security checklist. Based on the threat assessment, create a prioritized security controls checklist.

## Security Checklist Requirements

### Checklist Structure

For each control, provide:

#### Control: [Name]
- **Control ID**: SC-001 format
- **Category**: Access, Data, Infrastructure, Human, Compliance
- **NIST CSF Mapping**: Identify applicable function/Category
- **CIS Control**: Relevant CIS Control v8
- **Description**: What the control achieves
- **Implementation Steps**: Numbered steps to implement
- **Verification**: How to confirm implementation
- **Owner**: Who is responsible
- **Timeline**: Target completion
- **Effort**: Small/Medium/Large
- **Cost**: Low/Medium/High
- **Priority**: Critical/High/Medium/Low
- **Status**: Not Started/In Progress/Complete

### Critical Controls (Must Implement)

1. Multi-Factor Authentication
2. Endpoint Detection and Response
3. Access Review Process
4. Security Awareness Training
5. Incident Response Plan
6. Vulnerability Management
7. Logging and Monitoring
8. Data Backup Verification

### Priority Implementation Timeline

#### Phase 1: Foundation (0-30 days)
- Critical controls that can be implemented quickly
- Compliance requirements with deadlines
- Highest-impact quick wins

#### Phase 2: Core Security (30-90 days)
- Major security capabilities
- Process establishment
- Tool implementation

#### Phase 3: Maturation (90-180 days)
- Advanced controls
- Documentation completion
- Continuous improvement setup

### Compliance Mapping

Create a mapping showing how each control addresses:
- SOC 2 requirements
- Insurance requirements
- Customer requirements
- Regulatory requirements

### Resource Requirements

Summarize by phase:
- Total effort hours needed
- Budget requirements
- Tool/service costs
- Training needs

Save to outputs/security_checklist.md
```

---

## Prompt 4: Team-Specific Runbook

```
You are a security operations specialist creating a runbook for the specific team. Based on the team profile and security checklist, create an actionable runbook.

## Runbook Requirements

### Team Security Roles & Responsibilities

Define for each team member:
- Security domains they own
- Responsibilities during normal operations
- Responsibilities during incidents
- Escalation paths
- Decision authority

### Daily Operations

#### Daily Security Tasks (5 minutes)
- [ ] Review overnight security alerts
- [ ] Check for new vulnerability announcements
- [ ] Verify backup completion
- [ ] Review access request queue

#### Weekly Security Tasks (30 minutes)
- [ ] Review access review queue
- [ ] Check system logs for anomalies
- [ ] Review pending security tasks
- [ ] Update security documentation

#### Monthly Security Tasks (2 hours)
- [ ] Access review completion
- [ ] Security metrics compilation
- [ ] Policy review/update
- [ ] Training completion check

### Procedures

#### Procedure: Security Incident Recognition
**Purpose**: Help team members recognize potential security incidents

**Indicators of Concern**:
- Unexpected system behavior
- Unusual network activity
- Strange emails (phishing)
- Failed login attempts
- Unknown processes or connections
- Ransomware indicators

**Response Steps**:
1. Don't panic - most issues have innocent explanations
2. Document what you observed
3. Report to Alex immediately
4. Preserve evidence (screenshots, logs)
5. Follow instructions

#### Procedure: New Employee Onboarding Security
**Purpose**: Ensure new team members have appropriate security setup

**Before Day 1**:
- [ ] Create accounts with MFA enabled
- [ ] Provision access based on role
- [ ] Add to security training system

**Day 1**:
- [ ] Security awareness training assignment
- [ ] VPN/access setup
- [ ] Password manager account
- [ ] Welcome document with security expectations

**Week 1**:
- [ ] Role-specific security training
- [ ] System access walkthrough
- [ ] First security check-in

#### Procedure: Employee Offboarding Security
**Purpose**: Ensure departing employees have access revoked

**Notice Received**:
- [ ] Notify security owner immediately
- [ ] Document last day
- [ ] Plan access revocation timeline

**Last Day Actions**:
- [ ] Revoke all system access
- [ ] Recover company devices
- [ ] Transfer files per policy
- [ ] Disable accounts
- [ ] Retrieve credentials

**Post-Departure**:
- [ ] Access audit for any remaining access
- [ ] Review for data exfiltration
- [ ] Document lessons learned

#### Procedure: Phishing Response
**Purpose**: Guide response when phishing is suspected

**If You Suspect Phishing**:
1. Don't click any links or download attachments
2. Report to #security-alerts Slack channel
3. Capture screenshot if possible
4. Mark as junk in email client
5. Delete after reporting

**If You Clicked a Link or Entered Credentials**:
1. Disconnect from network immediately
2. Report to Alex directly (call/text)
3. Change the password you revealed
4. Document exactly what happened
5. Follow incident response if needed

#### Procedure: Vulnerability Response
**Purpose**: Guide response to vulnerability announcements

**When a Vulnerability is Announced**:
1. Assess if it affects our systems
2. Check for public exploit availability
3. Prioritize based on severity
4. Apply patches or mitigations
5. Document the response

**Severity Response Times**:
- Critical (CVSS 9-10): 24 hours
- High (CVSS 7-8): 72 hours
- Medium (CVSS 4-6): 2 weeks
- Low (CVSS 0-3): Next patch cycle

### Security Contacts

| Role | Contact | When to Contact |
|------|---------|-----------------|
| Security Lead | Alex | Security incidents, urgent concerns |
| Engineering Manager | Morgan | Process questions, resource requests |
| External Advisor | Casey | Complex security questions |
| Incident Escalation | CEO | Critical incidents, public-facing issues |

### Quick Reference Card

Create a one-page quick reference with:
- Key security contacts
- Common response procedures
- Important links
- Essential reminders

Save to outputs/team_runbook.md
```

---

## Prompt 5: HTML Security Playbook Dashboard

```
Create a comprehensive HTML security playbook dashboard that presents the entire security program in an interactive, professional format.

## Dashboard Sections Required

### 1. Playbook Header
- Title: "Security Playbook"
- Organization name: TechFlow Solutions
- Version/date
- Overall security maturity indicator

### 2. Executive Summary Card
- Organization risk profile
- Critical risks addressed
- Key compliance status
- Overall health score

### 3. Threat Overview Section
- Threat actors relevant to organization
- Top attack vectors
- Risk heat map or matrix
- Trend indicators

### 4. Compliance Status Dashboard
- SOC 2 readiness status
- Insurance requirement status
- Regulatory compliance status
- Audit readiness score

### 5. Security Controls Matrix
- Interactive table of controls
- Category breakdown (Access, Data, etc.)
- Priority indicators
- Status tracking
- Implementation progress

### 6. Implementation Roadmap
- Visual timeline (30/60/90/180 days)
- Phase breakdown with key milestones
- Dependencies between initiatives
- Progress tracking

### 7. Team Responsibilities
- Team member roles
- Ownership matrix
- Escalation paths
- Key contacts

### 8. Runbook Quick Links
- Daily/weekly/monthly tasks
- Incident recognition guide
- Phishing response procedure
- Emergency contacts

### 9. Metrics & KPIs
- Security metrics dashboard
- Key performance indicators
- Trend visualization
- Target vs. actual comparison

### 10. Resources Section
- Policy documents (links)
- Training materials (links)
- External resources
- Reference links

### 11. Document Information
- Created date
- Last updated
- Version
- Next review date

## Styling Requirements
- Clean, professional design
- Use CSS Grid or Flexbox for layout
- Mobile-responsive
- Print-friendly
- Professional color scheme (suggest: navy, greens for security theme)
- Readable typography with good hierarchy
- Visual hierarchy clear
- Card-based layout for sections

## Technical Requirements
- Single self-contained HTML file (no external dependencies)
- All CSS inline or in <style> block
- No JavaScript required (or inline only)
- Valid HTML5
- Works offline

## Interactivity (Recommended)
- Expandable/collapsible sections
- Progress indicators
- Checkable items (visual only)
- Tabbed navigation for different views
- Smooth transitions between sections

Save to: outputs/security_playbook.html

Before generating, describe your layout approach, color scheme, and any interactive elements you plan to include.
```

---

## Quick Prompt Reference

| #   | Prompt             | Output                 | Time  |
| --- | ------------------ | ---------------------- | ----- |
| 1   | Context Analysis   | security_context.md    | 4 min |
| 2   | Threat Assessment  | threat_assessment.md   | 5 min |
| 3   | Security Checklist | security_checklist.md  | 5 min |
| 4   | Team Runbook       | team_runbook.md        | 5 min |
| 5   | HTML Dashboard     | security_playbook.html | 5 min |

**Total estimated time**: 24-30 minutes
