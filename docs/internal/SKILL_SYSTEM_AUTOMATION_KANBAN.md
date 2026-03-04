# Skill System + Automation Upgrade Kanban

Last updated: 2026-03-04
Owner: Runtime + Control Panel

## Goal
Ship a control-panel-first Skill System with:
- Simple mode: pick flow + prompt + run
- Advanced mode: build custom skills/workflows
- Validation + evaluation loop before broad template rollout

## Status Legend
- [ ] Todo
- [~] In Progress
- [x] Done

## Phase 0 - Foundations and Tracking
- [x] Create execution kanban in `docs/internal`
- [~] Keep this board updated per commit

## Phase 1 - Backend Skill API Foundations
- [x] Add `GET /skills/catalog` (enriched metadata)
- [x] Add `POST /skills/validate` (SKILL.md + optional bundle validation)
- [x] Add `POST /skills/router/match` (goal -> skill match)
- [ ] Emit/update registry events where needed
- [x] Add unit tests for parsing/validation/router scoring

## Phase 2 - SDK / Client Contract Parity
- [x] Fix TypeScript `SkillLocation` parity with engine (`project|global`)
- [x] Add types for catalog/validate/router responses
- [x] Add `client.skills.catalog()`
- [x] Add `client.skills.validate()`
- [x] Add `client.skills.match()`

## Phase 3 - Control Panel Wizard Integration (Simple Mode)
- [x] In `AutomationsPage` Step 1, call router for top skill suggestion
- [x] Show matched skill and extracted params in wizard state
- [x] Keep fallback path to existing pack_builder prompt flow
- [x] Review step shows compile/validation summary (or fallback notes)

## Phase 4 - Evaluation Loop Scaffolding
- [x] Add `skill.eval.yaml` spec draft and validator stubs
- [x] Add baseline-vs-skill benchmark endpoint scaffold
- [x] Add trigger-eval endpoint scaffold and report schema
- [x] Add UI placeholder badges (`Validated` / `Not validated`)

## Phase 5 - Built-in Skill Templates and Advanced Mode
- [x] Land 10 default skills with `SKILL.md` + `workflow.yaml` + `automation.example.yaml`
- [x] Add advanced skill builder form + YAML toggle
- [x] Add "Generate Skill from Prompt" flow (gated by validation)

## Phase 6 - Production Hardening
- [x] Add generated-skill install API (`/skills/generate/install`) with validation + conflict policy handling
- [x] Add SDK method for generated-skill install
- [x] Add wizard install action for generated skill artifacts
- [x] Add server endpoint smoke coverage for new skill APIs
- [ ] Add persistent eval threshold policy enforcement for scheduled activation

## Commit Log
- [x] 2026-03-04: Add initial kanban board (`docs/internal/SKILL_SYSTEM_AUTOMATION_KANBAN.md`)
- [x] 2026-03-04: Add backend skill catalog/validate/router endpoints + SDK updates + tests
- [x] 2026-03-04: Integrate control-panel wizard skill routing (non-blocking fallback to pack builder)
- [x] 2026-03-04: Add skill evaluation scaffold endpoints + SDK methods + spec doc
- [x] 2026-03-04: Add 10 built-in skill templates
- [x] 2026-03-04: Add skills compile/generate endpoints + review compile UI + validation badge
- [x] 2026-03-04: Add advanced skill builder form + YAML artifact preview in wizard
- [ ] 2026-03-04: Add generate/install hardening and endpoint smoke coverage (pending commit)
