---
name: news-digest
description: Gather and summarize topic-specific news.
version: 1.0.0
tags: [news, research]
triggers:
  - daily news digest
  - summarize news about a topic
---

# Skill: News Digest

## Purpose

Create concise recurring news summaries for configured topics.

## Inputs

- topics
- recipients

## Agents

- finder
- summarizer

## Tools

- websearch
- webfetch
- text_summarize

## Workflow

1. Search recent sources
2. Filter for relevance
3. Summarize key points
4. Send digest

## Outputs

- topic news digest

## Schedule compatibility

- cron
- interval
- manual
