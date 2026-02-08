---
name: sales-daily-briefing
description: Start your day with a prioritized sales briefing. Works standalone when you tell me your meetings and priorities, supercharged when you connect your calendar, CRM, and email. Trigger with "morning briefing", "daily brief", "what's on my plate today", "prep my day", or "start my day".
---

# Daily Sales Briefing

Get a clear view of what matters most today. This skill works with whatever you tell me, and gets richer when you connect your tools.

## How It Works

```
┌─────────────────────────────────────────────────────────────────┐
│                      DAILY BRIEFING                              │
├─────────────────────────────────────────────────────────────────┤
│  ALWAYS (works standalone)                                       │
│  ✓ You tell me: today's meetings, key deals, priorities         │
│  ✓ I organize: prioritized action plan for your day             │
│  ✓ Output: scannable 2-minute briefing                          │
├─────────────────────────────────────────────────────────────────┤
│  SUPERCHARGED (when you connect your tools)                      │
│  + Calendar: auto-pull today's meetings with attendees          │
│  + CRM: pipeline alerts, tasks, deal health                     │
│  + Email: unread from key accounts, waiting on replies          │
│  + Enrichment: overnight signals on your accounts               │
└─────────────────────────────────────────────────────────────────┘
```

---

## Getting Started

When you run this skill, I'll ask for what I need:

**If no calendar connected:**

> "What meetings do you have today? (Just paste your calendar or list them)"

**If no CRM connected:**

> "What deals are you focused on this week? Any that need attention?"

**If you have connectors:**
I'll pull everything automatically and just show you the briefing.

---

## Connectors (Optional)

Connect your tools to supercharge this skill:

| Connector      | What It Adds                                                  |
| -------------- | ------------------------------------------------------------- |
| **Calendar**   | Today's meetings with attendees, times, and context           |
| **CRM**        | Open pipeline, deals closing soon, overdue tasks, stale deals |
| **Email**      | Unread from opportunity contacts, emails waiting on replies   |
| **Enrichment** | Overnight signals: funding, hiring, news on your accounts     |

> **No connectors?** No problem. Tell me your meetings and deals, and I'll create your briefing.

---

## Output Format

```markdown
# Daily Briefing | [Day, Month Date]

---

## #1 Priority

**[Most important thing to do today]**
[Why it matters and what to do about it]

---

## Today's Numbers

| Open Pipeline | Closing This Month | Meetings Today | Action Items |
| ------------- | ------------------ | -------------- | ------------ |
| $[X]          | $[X]               | [N]            | [N]          |

---

## Today's Meetings

### [Time] — [Company] ([Meeting Type])

**Attendees:** [Names]
**Context:** [One-line: deal status, last touch, what's at stake]
**Prep:** [Quick action before this meeting]

### [Time] — [Company] ([Meeting Type])

**Attendees:** [Names]
**Context:** [One-line context]
**Prep:** [Quick action]

_Run `sales-call-prep [company]` for detailed meeting prep_

---

## Pipeline Alerts
```
