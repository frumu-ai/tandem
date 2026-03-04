---
name: website-change-monitor
description: Watch important pages and notify when content changes.
version: 1.0.0
tags: [monitoring, web]
triggers:
  - monitor website changes
  - notify when this page changes
---

# Skill: Website Change Monitor

## Purpose

Detect significant content updates on tracked web pages.

## Inputs

- urls
- change sensitivity

## Agents

- fetcher
- diff-agent

## Tools

- webfetch
- email_send

## Workflow

1. Fetch page snapshot
2. Diff against previous snapshot
3. Score significance
4. Notify if threshold exceeded

## Outputs

- change alerts

## Schedule compatibility

- cron
- interval
