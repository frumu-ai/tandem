---
name: github-issue-reporter
description: Convert incidents into structured GitHub issues.
version: 1.0.0
tags: [github, incidents]
triggers:
  - file github issues from errors
  - create issues from alerts
---

# Skill: GitHub Issue Reporter

## Purpose

Create actionable GitHub issues from production incidents.

## Inputs

- incident summaries
- repository

## Agents

- incident-parser
- issue-writer

## Tools

- github.create_issue
- webfetch

## Workflow

1. Parse incident signal
2. Deduplicate against open issues
3. Draft issue body
4. Create issue

## Outputs

- github issue links

## Schedule compatibility

- interval
- manual
