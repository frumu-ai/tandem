---
name: mode-builder
description: Create Tandem custom modes through guided questions, then output one valid mode JSON object for preview and apply.
version: 1.0.0
tags:
  - tandem
  - modes
  - configuration
  - onboarding
compatibility: opencode
---

# Mode Builder

## Purpose

Help users create a safe, useful Tandem custom mode even if they are not technical.

## Core Rules

1. Ask follow-up questions with `ask_followup_question` whenever key details are missing.
2. Keep questions plain-language and low-jargon.
3. Default to safe behavior when uncertain.
4. Final answer must include exactly one JSON object in a fenced `json` block.
5. The JSON object must use only these fields:
   - `id`
   - `label`
   - `base_mode`
   - `icon`
   - `system_prompt_append`
   - `allowed_tools`
   - `edit_globs`
   - `auto_approve`
6. Do not include comments in JSON.
7. Do not include extra top-level keys like `scope`, `source`, or metadata.

## Valid `base_mode` values

- `immediate`
- `plan`
- `orchestrate`
- `coder`
- `ask`
- `explore`

## Practical Guidance

1. Recommend `ask` or `explore` for beginners.
2. Recommend `coder` only when the user explicitly wants code edits.
3. Use conservative `allowed_tools` first, then expand only if requested.
4. Use `edit_globs` whenever edits should be constrained.
5. Keep `auto_approve` set to `false` unless the user explicitly asks otherwise.

## Output Contract

When ready, output:

```json
{
  "id": "example-mode",
  "label": "Example Mode",
  "base_mode": "ask",
  "system_prompt_append": "Clear mode instructions.",
  "allowed_tools": ["read", "search", "glob"],
  "edit_globs": ["docs/**", "*.md"],
  "auto_approve": false
}
```

No additional text after the final JSON block.
