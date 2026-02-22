# README Translation Guide

This repo uses one canonical English README plus optional translated variants.

## Naming Convention

- English (source of truth): `README.md`
- Translations: `README.<lang>.md`
  - Examples: `README.es.md`, `README.pt-BR.md`, `README.ja.md`, `README.zh-CN.md`

## Recommended Workflow

1. Copy `README.md` to `README.<lang>.md`.
2. Translate content while preserving:
   - commands and code blocks
   - file paths
   - endpoint paths
   - environment variable names
3. Add the new language link under `README.md` in the **Language Options** section.
4. Keep translated README files in sync when major features change:
   - orchestration
   - automations
   - MCP connector behavior
   - installation/runtime requirements

## Style Guidance

- Prefer natural language over literal word-by-word translation.
- Keep product and API terms unchanged when needed for technical accuracy.
- If a concept has no direct equivalent, keep the English technical term and add a short clarification.
