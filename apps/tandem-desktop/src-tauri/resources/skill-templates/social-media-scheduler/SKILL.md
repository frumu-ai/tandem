---
name: social-media-scheduler
description: Draft and schedule social media posts.
version: 1.0.0
tags: [social, content]
triggers:
  - schedule social posts
  - plan weekly social content
---

# Skill: Social Media Scheduler

## Purpose

Generate and schedule channel-specific social content.

## Inputs

- channels
- content themes

## Agents

- writer
- scheduler

## Tools

- text_generate
- social_post

## Workflow

1. Draft post variants
2. Review against tone rules
3. Build posting schedule
4. Queue posts

## Outputs

- scheduled social posts

## Schedule compatibility

- cron
- manual
