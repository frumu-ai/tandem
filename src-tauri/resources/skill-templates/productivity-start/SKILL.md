---
name: productivity-start
description: Initialize the productivity system and open the dashboard
---

# Start Command

Initialize the task and memory systems, then open the unified dashboard.

## Instructions

### 1. Check What Exists

Check the working directory for:

- `TASKS.md` — task list
- `CONTEXT.md` — working memory
- `memory/` — deep memory directory
- `dashboard.html` — the visual UI

### 2. Create What's Missing

**If `TASKS.md` doesn't exist:** Create it with the standard template (see productivity-tasks skill). Place it in the current working directory.

**If `dashboard.html` doesn't exist:**
Copy it from the productivity pack. You can find it at:
`[Tandem Resource Path]/packs/productivity-pack/dashboard.html`
(Note: Adapt the path to the actual location where packs are stored in the Tandem environment, typically `src-tauri/resources/packs` in dev or similar in prod).

**If `CONTEXT.md` and `memory/` don't exist:** This is a fresh setup — after opening the dashboard, begin the memory bootstrap workflow (see below). Place these in the current working directory.

### 3. Open the Dashboard

Tell the user: "Dashboard is ready at `dashboard.html`. Open it from your file browser to get started."

### 4. Orient the User

If everything was already initialized:

```
Dashboard open. Your tasks and memory are both loaded.
- productivity-update to sync tasks and check memory
- productivity-update --comprehensive for a deep scan of all activity
```

If memory hasn't been bootstrapped yet, continue to step 5.

### 5. Bootstrap Memory (First Run Only)

Only do this if `CONTEXT.md` and `memory/` don't exist yet.

The best source of workplace language is the user's actual task list. Real tasks = real shorthand.

**Ask the user:**

```
Where do you keep your todos or task list? This could be:
- A local file (e.g., TASKS.md, todo.txt)
- An app (e.g. Asana, Linear, Jira, Notion, Todoist)
- A notes file

I'll use your tasks to learn your workplace shorthand.
```

**Once you have access to the task list:**

For each task item, analyze it for potential shorthand:

- Names that might be nicknames
- Acronyms or abbreviations
- Project references or codenames
- Internal terms or jargon

**For each item, decode it interactively:**

```
Task: "Send PSR to Todd re: Phoenix blockers"

I see some terms I want to make sure I understand:

1. **PSR** - What does this stand for?
2. **Todd** - Who is Todd? (full name, role)
3. **Phoenix** - Is this a project codename? What's it about?
```

Continue through each task, asking only about terms you haven't already decoded.

### 6. Optional Comprehensive Scan

After task list decoding, offer:

```
Do you want me to do a comprehensive scan of your messages, emails, and documents?
This takes longer but builds much richer context about the people, projects, and terms in your work.

Or we can stick with what we have and add context later.
```

**If they choose comprehensive scan:**

Gather data from available sources (Chat, Email, Docs, Calendar) and build a braindump of people, projects, and terms found.
