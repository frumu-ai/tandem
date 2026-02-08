---
name: enterprise-source-management
description: Manages connected MCP sources for enterprise search. Detects available sources and handles source priority ordering.
---

# Source Management

> If you see unfamiliar placeholders or need to check which tools are connected, check your MCP settings.

Knows what sources are available, helps connect new ones, and manages how sources are queried.

## Checking Available Sources

Determine which MCP sources are connected by checking available tools. Each source corresponds to a set of MCP tools:

| Source                | Key capabilities                                  |
| --------------------- | ------------------------------------------------- |
| **~~chat**            | Search messages, read channels and threads        |
| **~~email**           | Search messages, read individual emails           |
| **~~cloud storage**   | Search files, fetch document contents             |
| **~~project tracker** | Search tasks, typeahead search                    |
| **~~CRM**             | Query records (accounts, contacts, opportunities) |
| **~~knowledge base**  | Semantic search, keyword search                   |

If a tool prefix is available, the source is connected and searchable.

## Guiding Users to Connect Sources

When a user searches but has few or no sources connected:

```
You currently have [N] source(s) connected: [list].

To expand your search, you can connect additional sources in your MCP settings:
- ~~chat — messages, threads, channels
- ~~email — emails, conversations, attachments
- ~~cloud storage — docs, sheets, slides
- ~~project tracker — tasks, projects, milestones
- ~~CRM — accounts, contacts, opportunities
- ~~knowledge base — wiki pages, knowledge base articles

The more sources you connect, the more complete your search results.
```

When a user asks about a specific tool that is not connected:

```
[Tool name] isn't currently connected. To add it:
1. Open your MCP settings
2. Add the [tool] MCP server configuration
3. Authenticate when prompted

Once connected, it will be automatically included in future searches.
```

## Source Priority Ordering

Different query types benefit from searching certain sources first. Use these priorities to weight results, not to skip sources:

### By Query Type

**Decision queries** ("What did we decide..."):

```
1. ~~chat (conversations where decisions happen)
2. ~~email (decision confirmations, announcements)
3. ~~cloud storage (meeting notes, decision logs)
4. Wiki (if decisions are documented)
5. Task tracker (if decisions are captured in tasks)
```

**Status queries** ("What's the status of..."):

```
1. Task tracker (~~project tracker — authoritative status)
2. ~~chat (real-time discussion)
3. ~~cloud storage (status docs, reports)
4. ~~email (status update emails)
5. Wiki (project pages)
```

**Document queries** ("Where's the doc for..."):

```
1. ~~cloud storage (primary doc storage)
2. Wiki / ~~knowledge base (knowledge base)
3. ~~email (docs shared via email)
4. ~~chat (docs shared in channels)
5. Task tracker (docs linked to tasks)
```

**People queries** ("Who works on..." / "Who knows about..."):

```
1. ~~chat (message authors, channel members)
2. Task tracker (task assignees)
3. ~~cloud storage (doc authors, collaborators)
4. ~~CRM (account owners, contacts)
5. ~~email (email participants)
```

**Factual/Policy queries** ("What's our policy on..."):

```
1. Wiki / ~~knowledge base (official documentation)
```
