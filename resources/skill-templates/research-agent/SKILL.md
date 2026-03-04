---
name: research-agent
description: Perform grounded research and produce cited briefs.
version: 1.0.0
tags: [research, analysis]
triggers:
  - research this topic
  - create a research brief
---

# Skill: Research Agent

## Purpose

Produce structured research synthesis with citations.

## Inputs

- research query
- scope constraints

## Agents

- researcher
- verifier

## Tools

- websearch
- webfetch
- text_summarize

## Workflow

1. Gather candidate sources
2. Verify authority and freshness
3. Synthesize findings
4. Produce cited brief

## Outputs

- research report

## Schedule compatibility

- manual
- cron
