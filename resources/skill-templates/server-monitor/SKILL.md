---
name: server-monitor
description: Monitor service health checks and alert on failures.
version: 1.0.0
tags: [ops, monitoring]
triggers:
  - monitor my servers
  - alert on downtime
---

# Skill: Server Monitor

## Purpose

Detect service degradation quickly and notify operators.

## Inputs

- health endpoints
- escalation channel

## Agents

- checker
- incident-reporter

## Tools

- webfetch
- email_send

## Workflow

1. Probe endpoints
2. Evaluate status and latency
3. Aggregate failures
4. Send incident alert

## Outputs

- health incident alerts

## Schedule compatibility

- interval
- manual
