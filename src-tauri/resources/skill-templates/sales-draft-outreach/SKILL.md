---
name: sales-draft-outreach
description: Research a prospect then draft personalized outreach. Uses web research by default, supercharged with enrichment and CRM. Trigger with "draft outreach to [person/company]", "write cold email to [prospect]", "reach out to [name]".
---

# Draft Outreach

Research first, then draft. This skill never sends generic outreach - it always researches the prospect first to personalize the message. Works standalone with web search, supercharged when you connect your tools.

## Connectors (Optional)

| Connector      | What It Adds                                  |
| -------------- | --------------------------------------------- |
| **Enrichment** | Verified email, phone, background details     |
| **CRM**        | Prior relationship context, existing contacts |
| **Email**      | Create draft directly in your inbox           |

> **No connectors?** Web research works great. I'll output the email text for you to copy.

---

## How It Works

```
+------------------------------------------------------------------+
|                      DRAFT OUTREACH                               |
|                                                                   |
|  Step 1: RESEARCH (always happens first)                         |
|  - Web search (default)                                           |
|  - + Enrichment (if enrichment tools connected)                  |
|  - + CRM (if CRM connected)                                      |
|                                                                   |
|  Step 2: DRAFT (based on research)                               |
|  - Personalized opening (from research)                          |
|  - Relevant hook (their priorities)                              |
|  - Clear CTA                                                      |
|                                                                   |
|  Step 3: DELIVER (based on connectors)                           |
|  - Email draft (if email connected)                              |
|  - Copy for LinkedIn (always)                                    |
|  - Output to user (always)                                        |
+------------------------------------------------------------------+
```

---

## Output Format

```markdown
# Outreach Draft: [Person] @ [Company]

**Generated:** [Date] | **Research Sources:** [Web, Enrichment, CRM]

---

## Research Summary

**Target:** [Name], [Title] at [Company]
**Hook:** [Why reaching out now - the personalized angle]
**Goal:** [What you want from this outreach]

---

## Email Draft

**To:** [email if known, or "find email" note]
**Subject:** [Personalized subject line]

---

[Email body]

---

**Subject Line Alternatives:**

1. [Option 2]
2. [Option 3]

---

## LinkedIn Message (if no email)

**Connection Request (< 300 chars):**
[Short, no-pitch connection request]

**Follow-up Message (after connected):**
[Value-first message]

---

## Why This Approach

| Element | Based On                                  |
| ------- | ----------------------------------------- |
| Opening | [Research finding that makes it personal] |
| Hook    | [Their priority/pain point]               |
| Proof   | [Relevant customer story]                 |
| CTA     | [Low-friction ask]                        |

---
```
