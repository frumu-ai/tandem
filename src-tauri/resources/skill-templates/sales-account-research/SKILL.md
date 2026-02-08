---
name: sales-account-research
description: Research a company or person and get actionable sales intel. Works standalone with web search, supercharged when you connect enrichment tools or your CRM. Trigger with "research [company]", "look up [person]", "intel on [prospect]", "who is [name] at [company]", or "tell me about [company]".
---

# Account Research

Get a complete picture of any company or person before outreach. This skill always works with web search, and gets significantly better with enrichment and CRM data.

## How It Works

```
┌─────────────────────────────────────────────────────────────────┐
│                     ACCOUNT RESEARCH                             │
├─────────────────────────────────────────────────────────────────┤
│  ALWAYS (works standalone via web search)                        │
│  ✓ Company overview: what they do, size, industry               │
│  ✓ Recent news: funding, leadership changes, announcements      │
│  ✓ Hiring signals: open roles, growth indicators                │
│  ✓ Key people: leadership team from LinkedIn                    │
│  ✓ Product/service: what they sell, who they serve              │
├─────────────────────────────────────────────────────────────────┤
│  SUPERCHARGED (when you connect your tools)                      │
│  + Enrichment: verified emails, phone, tech stack, org chart    │
│  + CRM: prior relationship, past opportunities, contacts        │
└─────────────────────────────────────────────────────────────────┘
```

---

## Getting Started

Just tell me who to research:

- "Research Stripe"
- "Look up the CTO at Notion"
- "Intel on acme.com"
- "Who is Sarah Chen at TechCorp?"
- "Tell me about [company] before my call"

I'll run web searches immediately. If you have enrichment or CRM connected, I'll pull that data too.

---

## Connectors (Optional)

Connect your tools to supercharge this skill:

| Connector      | What It Adds                                                             |
| -------------- | ------------------------------------------------------------------------ |
| **Enrichment** | Verified emails, phone numbers, tech stack, org chart, funding details   |
| **CRM**        | Prior relationship history, past opportunities, existing contacts, notes |

> **No connectors?** No problem. Web search provides solid research for any company or person.

---

## Output Format

```markdown
# Research: [Company or Person Name]

**Generated:** [Date]
**Sources:** Web Search [+ Enrichment] [+ CRM]

---

## Quick Take

[2-3 sentences: Who they are, why they might need you, best angle for outreach]

---

## Company Profile

| Field            | Value                     |
| ---------------- | ------------------------- |
| **Company**      | [Name]                    |
| **Website**      | [URL]                     |
| **Industry**     | [Industry]                |
| **Size**         | [Employee count]          |
| **Headquarters** | [Location]                |
| **Founded**      | [Year]                    |
| **Funding**      | [Stage + amount if known] |
| **Revenue**      | [Estimate if available]   |

### What They Do

[1-2 sentence description of their business, product, and customers]

### Recent News

- **[Headline]** — [Date] — [Why it matters for your outreach]
- **[Headline]** — [Date] — [Why it matters]

### Hiring Signals

- [x] open roles in [Department]
- Notable: [Relevant roles like Engineering, Sales, AI/ML]
- Growth indicator: [Hiring velocity interpretation]

---
```
