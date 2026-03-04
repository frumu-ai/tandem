---
name: notion-weekly-report
description: Produce weekly progress summaries from Notion sources.
version: 1.0.0
tags: [notion, reporting]
triggers:
  - weekly notion report
  - summarize notion updates weekly
---

# Skill: Notion Weekly Report

## Purpose

Generate a weekly report from selected Notion pages and databases.

## Inputs

- notion sources
- report recipient

## Agents

- collector
- reporter

## Tools

- notion_read
- text_summarize
- email_send

## Workflow

1. Collect changes over the last week
2. Group by topic
3. Summarize progress and blockers
4. Deliver report

## Outputs

- weekly notion report

## Schedule compatibility

- cron
- manual
