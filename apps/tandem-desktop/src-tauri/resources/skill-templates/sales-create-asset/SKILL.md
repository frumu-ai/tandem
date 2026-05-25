---
name: sales-create-asset
description: Generate tailored sales assets (landing pages, decks, one-pagers, workflow demos) from your deal context. Describe your prospect, audience, and goal — get a polished, branded asset ready to share with customers.
---

# Create an Asset

**For Sales Teams Everywhere**

Generate professional, customer-ready sales assets in minutes. No design skills required.

---

## What It Does

This skill creates tailored sales assets by asking you about:

1. **Your prospect** — who they are, what you've discussed
2. **Your audience** — who's viewing, what they care about
3. **Your purpose** — what you want to achieve
4. **Your format** — how you want to present it

Then it researches, writes, designs, and builds a polished asset you can share with customers.

---

## Supported Formats

| Format                           | Best For                                | Output                                    |
| -------------------------------- | --------------------------------------- | ----------------------------------------- |
| **Interactive Landing Page**     | Exec meetings, value prop presentations | Multi-tab page with demos and calculators |
| **Deck-Style**                   | Formal presentations, large audiences   | Linear slides with navigation             |
| **One-Pager**                    | Leave-behinds, quick summaries          | Single-scroll executive summary           |
| **Workflow / Architecture Demo** | Technical deep-dives, POC proposals     | Interactive diagram with animated flow    |

---

## Quick Start

### Option 1: Simple prompt

```
Create an asset for Acme Corp
```

### Option 2: With context

```
Create an asset for Acme Corp. I met with their VP Engineering
last week - they're struggling with slow release cycles and
want to improve developer productivity. This is for a follow-up
with their technical team.
```

### Option 3: Workflow demo

```
I want to mock up a workflow showing how a customer would use
our product to automate their invoice processing. The flow is:
invoices come in via email → our AI extracts data → validates
against their ERP → flags exceptions for human review.
```

---

## Inputs at a Glance

| Input            | What to Provide                                         |
| ---------------- | ------------------------------------------------------- |
| **(a) Prospect** | Company, contacts, deal stage, pain points, transcripts |
| **(b) Audience** | Exec / Technical / Ops / Mixed + what they care about   |
| **(c) Purpose**  | Intro / Follow-up / Deep-dive / Alignment / POC / Close |
| **(d) Format**   | Landing page / Deck / One-pager / Workflow demo         |

---

## Phase 0: Context Detection & Input Collection

### Step 0.1: Detect Seller Context

From the user's email domain, identify what company they work for.

**Actions:**

1. Extract domain from user's email
2. Search: `"[domain]" company products services site:linkedin.com OR site:crunchbase.com`
3. Determine seller context:

| Scenario                             | Action                                               |
| ------------------------------------ | ---------------------------------------------------- |
| **Single-product company**           | Auto-populate seller context                         |
| **Multi-product company**            | Ask: "Which product or solution is this asset for?"  |
| **Consultant/agency/generic domain** | Ask: "What company or product are you representing?" |
| **Unknown/startup**                  | Ask: "Briefly, what are you selling?"                |

**Store seller context:**

```yaml
seller:
  company: "[Company Name]"
  product: "[Product/Service]"
  value_props:
    - "[Key value prop 1]"
    - "[Key value prop 2]"
    - "[Key value prop 3]"
  differentiators:
    - "[Differentiator 1]"
    - "[Differentiator 2]"
  pricing_model: "[If publicly known]"
```

**Persist to knowledge base** for future sessions. On subsequent invocations, confirm: "I have your seller context from last time — still selling [Product] at [Company]?"

### Step 0.2: Collect Prospect Context (a)

**Ask the user:**

| Field              | Prompt                                                                            | Required |
| ------------------ | --------------------------------------------------------------------------------- | -------- |
| **Company**        | "Which company is this asset for?"                                                | ✓ Yes    |
| **Key contacts**   | "Who are the key contacts? (names, roles)"                                        | No       |
| **Deal stage**     | "What stage is this deal?"                                                        | ✓ Yes    |
| **Pain points**    | "What pain points or priorities have they shared?"                                | No       |
| **Past materials** | "Upload any conversation materials (transcripts, emails, notes, call recordings)" | No       |

### Step 0.3: Collect Audience Context (b)

**Ask the user:**

| Field               | Prompt                                                                | Required |
| ------------------- | --------------------------------------------------------------------- | -------- |
| **Audience type**   | "Who's viewing this?"                                                 | ✓ Yes    |
| **Specific roles**  | "Any specific titles to tailor for? (e.g., CTO, VP Engineering, CFO)" | No       |
| **Primary concern** | "What do they care most about?"                                       | ✓ Yes    |

---

## Output

- Self-contained HTML file
- Works offline
- Host anywhere (Netlify, Vercel, GitHub Pages, etc.)
