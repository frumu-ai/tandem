---
name: dev-agent
description: Analyze repositories, implement code changes, run tests, and optionally commit.
version: 1.0.0
tags: [development, coding, testing]
triggers:
  - fix this bug in my repo
  - implement this feature
---

# Skill: Dev Agent

## Purpose

Run an end-to-end coding workflow in a repository.

## Inputs

- repository path
- task prompt
- commit enabled

## Agents

- planner
- worker
- reviewer

## Tools

- filesystem
- shell
- git
- webfetch

## Workflow

1. Analyze repository and task
2. Plan changes
3. Implement and run tests
4. Review outcomes
5. Optionally commit

## Outputs

- code diff summary
- test report
- commit hash (optional)

## Schedule compatibility

- manual
