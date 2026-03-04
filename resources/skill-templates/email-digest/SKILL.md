---
name: email-digest
description: Summarize important unread emails each morning.
version: 1.0.0
tags: [email, productivity, digest]
triggers:
  - check my email every morning
  - send me a daily inbox summary
---

# Skill: Email Digest

## Purpose

Summarize important unread emails and send a concise daily digest.

## Inputs

- inbox account
- recipient

## Agents

- email-reader
- summarizer

## Tools

- gmail_read
- text_summarize
- email_send

## Workflow

1. Fetch unread emails
2. Rank importance
3. Summarize top items
4. Send digest

## Outputs

- daily inbox digest

## Schedule compatibility

- cron
- interval
- manual
