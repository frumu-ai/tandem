---
name: sales-call-prep
description: Prepare for a sales call with account context, attendee research, and suggested agenda. Works standalone with user input and web research, supercharged when you connect your CRM, email, chat, or transcripts. Trigger with "prep me for my call with [company]", "I'm meeting with [company] prep me", "call prep [company]", or "get me ready for [meeting]".
---

# Call Prep

Get fully prepared for any sales call in minutes. This skill works with whatever context you provide, and gets significantly better when you connect your sales tools.

## How It Works

```
┌─────────────────────────────────────────────────────────────────┐
│                        CALL PREP                                 │
├─────────────────────────────────────────────────────────────────┤
│  ALWAYS (works standalone)                                       │
│  ✓ You tell me: company, meeting type, attendees                │
│  ✓ Web search: recent news, funding, leadership changes         │
│  ✓ Company research: what they do, size, industry               │
│  ✓ Output: prep brief with agenda and questions                 │
├─────────────────────────────────────────────────────────────────┤
│  SUPERCHARGED (when you connect your tools)                      │
│  + CRM: account history, contacts, opportunities, activities    │
│  + Email: recent threads, open questions, commitments           │
│  + Chat: internal discussions, colleague insights               │
│  + Transcripts: prior call recordings, key moments              │
│  + Calendar: auto-find meeting, pull attendees                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Getting Started

When you run this skill, I'll ask for what I need:

**Required:**

- Company or contact name
- Meeting type (discovery, demo, negotiation, check-in, etc.)

**Helpful if you have it:**

- Who's attending (names and titles)
- Any context you want me to know (paste prior notes, emails, etc.)

If you've connected your CRM, email, or other tools, I'll pull context automatically and skip the questions.

---

## Connectors (Optional)

Connect your tools to supercharge this skill:

| Connector       | What It Adds                                                                 |
| --------------- | ---------------------------------------------------------------------------- |
| **CRM**         | Account details, contact history, open deals, recent activities              |
| **Email**       | Recent threads with the company, open questions, attachments shared          |
| **Chat**        | Internal chat discussions (e.g. Slack) about the account, colleague insights |
| **Transcripts** | Prior call recordings, topics covered, competitor mentions                   |
| **Calendar**    | Auto-find the meeting, pull attendees and description                        |

> **No connectors?** No problem. Just tell me about the meeting and paste any context you have. I'll research the rest.

---

## Output Format

```markdown
# Call Prep: [Company Name]

**Meeting:** [Type] — [Date/Time if known]
**Attendees:** [Names with titles]
**Your Goal:** [What you want to accomplish]

---

## Account Snapshot

| Field          | Value                                          |
| -------------- | ---------------------------------------------- |
| **Company**    | [Name]                                         |
| **Industry**   | [Industry]                                     |
| **Size**       | [Employees / Revenue if known]                 |
| **Status**     | [New prospect / Active opportunity / Customer] |
| **Last Touch** | [Date and summary]                             |

---

## Who You're Meeting

### [Name] — [Title]

- **Background:** [Career history, education if found]
- **LinkedIn:** [URL]
- **Role in Deal:** [Decision maker / Champion / Evaluator / etc.]
- **Last Interaction:** [Summary if known]
- **Talking Point:** [Something personal/professional to reference]

[Repeat for each attendee]

---
```
