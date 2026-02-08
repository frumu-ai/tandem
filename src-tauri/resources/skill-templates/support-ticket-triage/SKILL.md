---
name: support-ticket-triage
description: Triage and prioritize a support ticket or customer issue. Categorize issues, assign priority (P1-P4), and recommend routing.
---

# Support Ticket Triage

> If you see unfamiliar placeholders or need to check which tools are connected, please ask about available integrations.

Categorize, prioritize, and route an incoming support ticket or customer issue. Produces a structured triage assessment with a suggested initial response.

## Usage

Provide the ticket text, customer message, or issue description.

Examples:

- "Customer says their dashboard has been showing a blank page since this morning"
- "I was charged twice for my subscription this month"
- "User can't connect their SSO — getting a 403 error on the callback URL"

## Workflow

### 1. Parse the Issue

Read the input and extract:

- **Core problem**: What is the customer actually experiencing?
- **Symptoms**: What specific behavior or error are they seeing?
- **Customer context**: Who is this? Any account details, plan level, or history available?
- **Urgency signals**: Are they blocked? Is this production? How many users affected?

### 2. Categorize and Prioritize

Assign a **primary category** and **priority**:

| Category            | Description                                               |
| ------------------- | --------------------------------------------------------- |
| **Bug**             | Product is behaving incorrectly or unexpectedly           |
| **How-to**          | Customer needs guidance on using the product              |
| **Feature request** | Customer wants a capability that doesn't exist            |
| **Billing**         | Payment, subscription, invoice, or pricing issues         |
| **Account**         | Account access, permissions, settings, or user management |
| **Integration**     | Issues connecting to third-party tools or APIs            |
| **Security**        | Security concerns, data access, or compliance questions   |
| **Data**            | Data quality, migration, import/export issues             |
| **Performance**     | Speed, reliability, or availability issues                |

| Priority          | Criteria                                       | SLA Expectation |
| ----------------- | ---------------------------------------------- | --------------- |
| **P1 — Critical** | Production down, data loss, security breach    | 1 hour          |
| **P2 — High**     | Major feature broken, workflow blocked         | 4 hours         |
| **P3 — Medium**   | Feature partially broken, workaround available | 1 business day  |
| **P4 — Low**      | Minor inconvenience, question, feature request | 2 business days |

### 3. Determine Routing

Recommend which team or queue should handle this:

- **Tier 1**: How-to, known issues, billing, password resets
- **Tier 2**: Bugs requiring investigation, complex config, integrations
- **Engineering**: Confirmed bugs requiring code fixes, infrastructure
- **Product**: Feature requests, design decisions
- **Security**: Data exposure, vulnerabilities
- **Billing/Finance**: Refunds, contract disputes

### 4. Generate Triage Output

```markdown
## Triage: [One-line issue summary]

**Category:** [Primary] / [Secondary if applicable]
**Priority:** [P1-P4] — [Brief justification]
**Product area:** [Area/team]

### Issue Summary

[2-3 sentence summary of what the customer is experiencing]

### Key Details

- **Customer:** [Name/account if known]
- **Impact:** [Who and what is affected]
- **Workaround:** [Available / Not available / Unknown]

### Routing Recommendation

**Route to:** [Team or queue]
**Why:** [Brief reasoning]

### Suggested Initial Response

[Draft first response to the customer — acknowledge the issue, set expectations, provide workaround if available.]
```
