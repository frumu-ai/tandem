---
name: competitor-price-tracker
description: Monitor competitor product pages and alert when prices change.
version: 1.0.0
tags: [monitoring, pricing]
triggers:
  - monitor competitor prices
  - alert me when pricing changes
---

# Skill: Competitor Price Tracker

## Purpose

Track target pricing pages and notify on meaningful price changes.

## Inputs

- product urls
- alert target

## Agents

- page-fetcher
- price-comparator

## Tools

- webfetch
- email_send

## Workflow

1. Fetch tracked pages
2. Extract current price
3. Compare with last known value
4. Send alert on change

## Outputs

- price change alerts

## Schedule compatibility

- cron
- interval
