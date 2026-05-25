---
name: enterprise-knowledge-synthesis
description: Combines search results from multiple sources into coherent, deduplicated answers with source attribution.
---

# Knowledge Synthesis

The last mile of enterprise search. Takes raw results from multiple sources and produces a coherent, trustworthy answer.

## The Goal

Transform this:

```
~~chat result: "Sarah said in #eng: 'let's go with REST, GraphQL is overkill for our use case'"
~~email result: "Subject: API Decision — Sarah's email confirming REST approach with rationale"
~~cloud storage result: "API Design Doc v3 — updated section 2 to reflect REST decision"
~~project tracker result: "Task: Finalize API approach — marked complete by Sarah"
```

Into this:

```
The team decided to go with REST over GraphQL for the API redesign. Sarah made the
call, noting that GraphQL was overkill for the current use case. This was discussed
in #engineering on Tuesday, confirmed via email Wednesday, and the design doc has
been updated to reflect the decision. The related ~~project tracker task is marked complete.

Sources:
- ~~chat: #engineering thread (Jan 14)
- ~~email: "API Decision" from Sarah (Jan 15)
- ~~cloud storage: "API Design Doc v3" (updated Jan 15)
- ~~project tracker: "Finalize API approach" (completed Jan 15)
```

## Deduplication

### Cross-Source Deduplication

The same information often appears in multiple places. Identify and merge duplicates:

**Signals that results are about the same thing:**

- Same or very similar text content
- Same author/sender
- Timestamps within a short window (same day or adjacent days)
- References to the same entity (project name, document, decision)
- One source references another ("as discussed in ~~chat", "per the email", "see the doc")

**How to merge:**

- Combine into a single narrative item
- Cite all sources where it appeared
- Use the most complete version as the primary text
- Add unique details from each source

### Deduplication Priority

When the same information exists in multiple sources, prefer:

```
1. The most complete version (fullest context)
2. The most authoritative source (official doc > chat)
3. The most recent version (latest update wins for evolving info)
```

### What NOT to Deduplicate

Keep as separate items when:

- The same topic is discussed but with different conclusions
- Different people express different viewpoints
- The information evolved meaningfully between sources (v1 vs v2 of a decision)
- Different time periods are represented

## Citation and Source Attribution

Every claim in the synthesized answer must be attributable to a source.

### Attribution Format

Inline for direct references:

```
Sarah confirmed the REST approach in her email on Wednesday.
The design doc was updated to reflect this (~~cloud storage: "API Design Doc v3").
```

Source list at the end for completeness:

```
Sources:
- ~~chat: #engineering discussion (Jan 14) — initial decision thread
- ~~email: "API Decision" from Sarah Chen (Jan 15) — formal confirmation
- ~~cloud storage: "API Design Doc v3" last modified Jan 15 — updated specification
```

### Attribution Rules

- Always name the source type (~~chat, ~~email, ~~cloud storage, etc.)
- Include the specific location (channel, folder, thread)
- Include the date or relative time
- Include the author when relevant
- Include document/thread titles when available
- For ~~chat, note the channel name
- For ~~email, note the subject line and sender
- For ~~cloud storage, note the document title
