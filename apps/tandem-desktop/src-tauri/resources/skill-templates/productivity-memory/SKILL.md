---
name: productivity-memory
description: Two-tier memory system that makes the AI a true workplace collaborator. Decodes shorthand, acronyms, nicknames, and internal language so the AI understands requests like a colleague would. Uses CONTEXT.md for working memory and a memory/ directory for the full knowledge base.
---

# Memory Management

Memory makes the AI your workplace collaborator - someone who speaks your internal language.

## The Goal

Transform shorthand into understanding:

```
User: "ask todd to do the PSR for oracle"
              ↓ AI Decodes
"Ask Todd Martinez (Finance lead) to prepare the Pipeline Status Report
 for the Oracle Systems deal ($2.3M, closing Q2)"
```

Without memory, that request is meaningless. With memory, the AI knows:

- **todd** → Todd Martinez, Finance lead, prefers Slack
- **PSR** → Pipeline Status Report (weekly sales doc)
- **oracle** → Oracle Systems deal, not the company

## Architecture

```
CONTEXT.md           ← Hot cache (~30 people, common terms)
memory/
  glossary.md      ← Full decoder ring (everything)
  people/          ← Complete profiles
  projects/        ← Project details
  context/         ← Company, teams, tools
```

**CONTEXT.md (Hot Cache):**

- Top ~30 people you interact with most
- ~30 most common acronyms/terms
- Active projects (5-15)
- Your preferences
- **Goal: Cover 90% of daily decoding needs**

**memory/glossary.md (Full Glossary):**

- Complete decoder ring - everyone, every term
- Searched when something isn't in CONTEXT.md
- Can grow indefinitely

**memory/people/, projects/, context/:**

- Rich detail when needed for execution
- Full profiles, history, context

## Lookup Flow

```
User: "ask todd about the PSR for phoenix"

1. Check CONTEXT.md (hot cache)
   → Todd? ✓ Todd Martinez, Finance
   → PSR? ✓ Pipeline Status Report
   → Phoenix? ✓ DB migration project

2. If not found → search memory/glossary.md
   → Full glossary has everyone/everything

3. If still not found → ask user
   → "What does X mean? I'll remember it."
```

This tiered approach keeps CONTEXT.md lean (~100 lines) while supporting unlimited scale in memory/.

## File Locations

- **Working memory:** `CONTEXT.md` in current working directory
- **Deep memory:** `memory/` subdirectory

## Working Memory Format (CONTEXT.md)

Use tables for compactness. Target ~50-80 lines total.

```markdown
# Memory

## Me

[Name], [Role] on [Team]. [One sentence about what I do.]

## People

| Who       | Role                               |
| --------- | ---------------------------------- |
| **Todd**  | Todd Martinez, Finance lead        |
| **Sarah** | Sarah Chen, Engineering (Platform) |
| **Greg**  | Greg Wilson, Sales                 |

→ Full list: memory/glossary.md, profiles: memory/people/

## Terms

| Term    | Meaning                  |
| ------- | ------------------------ |
| PSR     | Pipeline Status Report   |
| P0      | Drop everything priority |
| standup | Daily 9am sync           |
```
