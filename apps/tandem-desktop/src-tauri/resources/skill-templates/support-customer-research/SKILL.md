---
name: support-customer-research
description: Research customer questions by searching across documentation, knowledge bases, and connected sources. Synthesize a confidence-scored answer.
---

# Support Customer Research

Conduct multi-source research to answer customer questions, investigate account contexts, and build comprehensive understanding of customer situations. Prioritize authoritative sources, synthesize across inputs, and clearly communicate confidence levels.

## Research Methodology

1. **Understand the Question**: Is it factual, contextual, or exploratory? Who is the audience?
2. **Plan Search Strategy**: Map question to source types (Docs, CRM, Slack, Web).
3. **Execute Searches**: Search sources in priority order. Cross-reference findings.
4. **Synthesize**: Combine findings, check for contradictions.
5. **Present**: Cite sources and note confidence level.

## Source Prioritization

| Tier                     | Sources                                                    | Confidence      |
| ------------------------ | ---------------------------------------------------------- | --------------- |
| **1. Official Internal** | Product docs, Knowledge base, Policies, Roadmap            | **High**        |
| **2. Org Context**       | CRM records, Support tickets, Internal docs, Meeting notes | **Medium-High** |
| **3. Team Comms**        | Chat history, Email threads, Calendar notes                | **Medium**      |
| **4. External**          | Web search, Forums, 3rd-party docs, News                   | **Low-Medium**  |
| **5. Inferred**          | Similar situations, Analogous customers, Best practices    | **Low**         |

## Answer Synthesis

Always assign and communicate a confidence level:

- **High Confidence**: Confirmed by official docs or multiple sources. Current info.
- **Medium Confidence**: Based on team chat, older tickets, or single unverified source.
- **Low Confidence**: Inferred, based on external info, or conflicting data.

## Output Format

```markdown
## Answer Summary

[Direct answer to the question]

## Details & Evidence

- [Key point 1] (Source: [Link/Ref])
- [Key point 2] (Source: [Link/Ref])

## Context/Background

[Additional relevant information found]

## Confidence: [High/Medium/Low]

[Explanation of confidence level and any caveats]
```
