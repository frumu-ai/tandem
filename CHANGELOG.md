# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.8] - Unreleased

### Added

- **Studio workflow builder in the control panel**:
  - added a new top-level `Studio` page for template-first multi-agent workflow design
  - added starter workflow templates, editable per-agent role prompts, stage/dependency editing, and saved Studio workflow cards
  - added a shared workspace browser in Studio so workflow roots use the same folder-picker flow as the rest of the app
- **Workflow run recovery controls and richer node metadata**:
  - added `Continue` / `Continue From Here` for blocked `automation_v2` runs and `Retry` / `Retry Workflow` actions in the Run Debugger
  - added semantic node output metadata for `status`, `blocked_reason`, `approved`, tool telemetry, and artifact-validation results
- **Repo coding backlog task operations**:
  - projected coding backlog items can now be claimed and manually requeued through `automation_v2` run APIs instead of remaining read-only debugger projections
  - added stale-lease handling in the shared context-task runtime so expired `in_progress` backlog tasks automatically return to the runnable queue before the next claim
  - surfaced backlog claim/requeue controls plus lease-expiry and stale-state visibility in the control-panel Run Debugger
- **Agent context component manifests**:
  - added first-party component manifests for Tandem engine, desktop, TUI, control panel, and SDK clients under `manifests/components/`
  - bundled matching agent-context manifest copies into the desktop resources so runtime agents can use the same conservative component map

### Changed

- **Workflow board and debugger usability**:
  - moved the workflow board onto its own full-width row in the Run Debugger
  - made desktop workflow lanes horizontally scrollable with jump-to-active controls instead of clipping off-screen columns
  - surfaced offered/executed tools, workspace-inspection usage, web-research usage, and artifact-validation details directly in task inspection
  - added richer coding-task verification details in the Run Debugger, including per-step verification results and `done` status for successfully verified code tasks
  - failed automation runs now preserve the latest linked session id so the debugger can still link back to the most recent transcript context
  - workflow node outputs now expose typed stability metadata including `workflow_class`, `phase`, `failure_kind`, and artifact-candidate summaries so the debugger is driven by backend state instead of transcript inference
  - workflow runs now emit typed node-scoped lifecycle events such as `workflow_state_changed`, `artifact_accepted`, `artifact_rejected`, `research_coverage_failed`, and verification/repair events so stability state can be consumed from shared runtime history
  - desktop/TUI coder run summaries now include typed workflow stability fields and recent workflow events per task so the developer inspector follows the same backend contract as the control panel
  - Studio saved-workflow cards now surface the latest run’s typed stability snapshot so authoring views can see recent status, phase, and failure-kind state without leaving Studio
  - artifact finalization now scores verified output, session writes, and preexisting output so the strongest candidate wins deterministically instead of depending on late placeholder-phrase rejection
  - control-panel Studio and Run Debugger now consume shared workflow-stability selectors instead of duplicating node-output and lifecycle parsing per page
  - more control-panel workflow surfaces now use the shared workflow-stability selector layer for session IDs, latest stability snapshots, node-output text, and telemetry extraction
  - desktop agent-automation views now reuse the shared coder workflow-run parsers for session IDs and node outputs instead of duplicating local extraction logic
- **Workflow Studio model configuration**:
  - replaced free-text workflow model inputs with provider-backed selectors
  - added an optional shared-model mode so one provider/model choice can be applied across every workflow agent for cheaper runs
- **Automation execution now prefers deterministic file-backed workflow behavior**:
  - `automation_v2` nodes now run with explicit required tool sets instead of relying on the generic auto-router alone
  - normalized workflow tool exposure so `read` implies `glob` for workspace discovery
  - added prewrite requirements for workspace inspection and web research before file-finalization retries narrow down to write-only
  - write-required workflow retries now force the first missing artifact write instead of continuing to offer discovery tools before any declared output exists
  - coding workflow scheduling now filters overlapping write scopes out of the same runnable batch so parallel code tasks do not edit the same repo area at once by default

### Fixed

- **Artifact integrity for file-backed workflows**:
  - fixed agents overwriting declared artifacts with placeholder status notes such as “completed previously” / “preserving file creation requirement”
  - fixed declared workflow artifacts being replaced by stray touch/status/marker files created by the model
  - fixed substantive blocked briefs being deleted just because the node was semantically blocked; blocked research now preserves useful artifact content for inspection
  - fixed fresh workflow runs wiping prior declared outputs before a replacement artifact existed; failed reruns now preserve the last substantive file instead of leaving the workspace empty
  - added recovery from in-session write history so earlier substantive file content can be restored when a later placeholder overwrite wins the final on-disk write
  - cleared stale descendant outputs on blocked-step continue/retry so subtree recovery no longer reuses bad artifacts
- **Workflow blocking and runtime semantics**:
  - fixed blocked node outcomes to stop downstream steps consistently instead of letting draft/review/publish continue with blocked handoff artifacts
  - fixed review nodes with `approved: false` to propagate a blocked status cleanly through the automation runtime and debugger
  - fixed file-backed research briefs being marked `completed` when they cited workspace sources without ever calling `read`; these now block even if the model writes a polished “source-backed” brief
  - fixed timed-out `websearch` attempts still counting as successful current-market research; research now blocks when required web research only timed out or returned no usable result
  - brief/research nodes now carry stricter prewrite requirements for concrete `read`, successful web research, one automatic repair retry, and coverage-aware validation metadata before they finalize as blocked
  - research artifact validation now records actual `read` paths, discovered relevant files, `Files reviewed` coverage backed by reads, unreviewed relevant files, web-research success, and repair-pass state so blocked runs show the real failure cause
  - code workflows with multi-step verification plans now emit explicit verification outcomes; partial verification blocks completion, failed verification emits `verify_failed`, and fully verified code tasks finish as `done`
  - fixed `/workspace/...` tool paths so file-backed workflow nodes resolve against the actual workspace root instead of failing on a fake alias
- **Saved Studio workflow deletion and restart persistence**:
  - fixed saved Studio workflows reappearing after engine restart by cascading automation deletion into persisted `automation_v2` run history so old run snapshots cannot recreate deleted workflows
- **Control-panel source-install docs**:
  - fixed the control-panel README examples so service commands work both from the repo root and from inside `packages/tandem-control-panel`
- **Channel and browser runtime defaults**:
  - channel-created sessions now pre-approve browser tools and `mcp*` tool namespaces so channel users do not get stuck on invisible approval prompts
  - browser sidecar env flags now use clap-friendly boolean strings and browser-open requests normalize blank profile ids instead of passing malformed values downstream
- **Permission matching for tool namespaces**:
  - fixed permission evaluation so wildcard permission names like `mcp*` match namespaced tool ids instead of only exact permission strings

## [0.4.7] - Released

### Added

- **Lossless cross-session channel memory archival**:
  - added automatic archival of completed Telegram/Discord/Slack user-visible exchanges into global retrieval memory while keeping full raw transcripts in ordinary session storage
  - added provenance-based dedupe for archived `chat_exchange` rows so repeated retries of the same exchange do not duplicate global memory indefinitely
  - added default memory-tool permissions for newly created channel sessions so channel bots can actually call `memory_search`, `memory_store`, and `memory_list`
- **Channel workflow-planning slash commands**:
  - added grouped `/help` output with topic help via `/help schedule`
  - added `/schedule plan`, `/schedule show`, `/schedule edit`, `/schedule reset`, and `/schedule apply` so channel users can draft and save automations without leaving Telegram/Discord/Slack
  - added workflow-planner workspace reuse so slash-command drafts inherit the active session workspace root when available
- **Expanded namespaced operator commands for channels**:
  - added `/automations`, `/runs`, `/memory`, `/workspace`, `/mcp`, `/packs`, and `/config` command families on top of existing engine APIs
  - added topic help via `/help automations`, `/help runs`, `/help memory`, `/help workspace`, `/help mcp`, `/help packs`, and `/help config`
  - added explicit `--yes` confirmation for destructive channel commands such as automation delete, memory delete, and pack uninstall
- **Storage-root standardization guidance and diagnostics**:
  - added startup diagnostics that warn operators when `TANDEM_STATE_DIR` and `TANDEM_MEMORY_DB_PATH` point at different roots
  - added focused regression coverage for canonical memory DB path resolution and channel archival dedupe behavior

### Changed

- **Standard install storage layout now converges on one Tandem root**:
  - shared path resolution now falls back to `TANDEM_STATE_DIR` before OS defaults, so standard installs keep memory, config, logs, and session storage under one canonical Tandem directory
  - control-panel setup helpers, VPS examples, and quickstart scripts no longer generate `TANDEM_MEMORY_DB_PATH` by default
  - standard OS defaults remain:
    - Linux: `~/.local/share/tandem/`
    - macOS: `~/Library/Application Support/tandem/`
    - Windows: `%APPDATA%\\tandem\\`

### Fixed

- **Memory-tool reliability and retrieval correctness**:
  - fixed public memory tool calls being able to override the backing SQLite path via arbitrary `db_path` values
  - fixed `memory_list tier=global` decoding against `global_memory_chunks`, including the broken `token_count` / column-index path
  - fixed channel archival writing to a different SQLite file than the one used by `memory_search` and `memory_list`
  - fixed fresh channel sessions failing to access memory because memory tools were missing from the default permission allowlist

## [0.4.6] - Released

### Added

- **Advanced Swarm Builder across desktop and control panel**:
  - added a new mission-oriented advanced builder on top of `AutomationV2Spec` for coordinated multi-agent workloads
  - added authored mission blueprints with mission goal, shared context, orchestrator selection, workstreams, dependencies, output contracts, review stages, and approval gates
  - added first-class PM semantics for advanced missions, including phases, lanes, priorities, milestones, and gate metadata
  - added advanced-builder compile preview, validation warnings, graph summaries, and grouped lane/phase visualization
  - added native control-panel support for the advanced builder so web and desktop can both create and edit advanced mission automations
  - added in-product operator guidance for the web advanced builder, including a how-it-works modal, inline help text, and starter mission presets
  - added external starter-mission preset files so advanced builder examples live as content instead of hardcoded UI data
- **Desktop Coder workspace for coding-swarm creation and operations**:
  - turned the desktop `Developer` surface into `Coder` and made it the visible home for coding swarms
  - added a dedicated Coder workspace with `Create` and `Runs` tabs instead of a legacy run inspector-only screen
  - embedded coding-swarm creation in Coder on top of the existing advanced mission builder and `MissionBlueprint -> AutomationV2Spec` flow
  - added coding preset selection, user-repo context surfacing, and a lightweight saved-template shelf in the Coder create flow
  - added automation-backed Coder run projection so coder-tagged `AutomationV2` runs appear directly in the Coder run list
  - added operator tabs for coder runs covering overview, transcripts, context, artifacts, and memory
  - added direct cross-links from Coder runs into Agent Automation and Command Center

### Changed

- **Advanced mission execution, recovery, and observability**:
  - compiled advanced missions into richer `AutomationV2Spec` metadata while keeping execution on the existing automation runtime
  - added mission-wide inheritance so workstreams receive the global brief, success criteria, shared context, scoped assignment, dependency context, and output expectations
  - improved automation runtime scheduling with phase-open semantics, runnable frontier shaping, and priority-aware ordering without violating dependency legality
  - added advanced-automation reopen/edit continuity, including reconstruction of older sparse `mission_blueprint` records from compiled automation data
  - expanded control-panel graph preview and scope editing so per-step tools and MCP overrides are easier to inspect and edit
  - clarified Phase 6 preset/template status: advanced mission starter presets remain a local bundled shelf pending stronger persisted preset promotion, while agent-team templates already use workspace-backed persisted storage
- **Coder now reuses the existing mission and automation stack instead of acting as a separate coding surface**:
  - tagged coder-originated missions with typed coder metadata instead of inventing a second coding executor or run model
  - wired the desktop to consume explicit backend-linked context run IDs for automation-backed coder runs instead of synthesizing them locally
  - resolved user repo context from the active user project path, including repo root, remote slug, current branch, and default branch, and merged that context into coder mission metadata
  - extracted shared coder run list, run detail, and action toolbar components so the Coder workspace is not glued to full-page legacy surfaces

### Fixed

- **Automation V2 control, failure handling, and run diagnostics**:
  - added explicit operator-stop, guardrail-stop, pause, resume, and recovery semantics for advanced automation runs, including tighter active session and instance cleanup
  - hardened branch-local repair, gate rework, and pause/recovery flows so completed sibling branches remain intact and blocked fan-in is recomputed coherently
  - added structured node lifecycle events, repair history, milestone promotion history, and richer per-step diagnostics in automation run inspection
  - fixed advanced-builder schedule payloads to use the server-required tagged `misfire_policy` shape
  - fixed external mission preset loading in the control panel
  - fixed an engine panic caused by malformed one-character quoted scalar values during automation execution and converted node panics into normal run failures instead of leaving runs deceptively pending
  - fixed channel bots silently ignoring inbound messages when control-panel channel settings were saved with a blank `allowed_users` field; blank allowlists now normalize to wildcard `["*"]` in both the control panel and server runtime config
- **Coder workspace data and operator wiring**:
  - fixed the blank or underpowered desktop coder screen by routing it through a real creation-and-operations workspace
  - fixed automation-backed coder detail views to rely on explicit backend context-run linkage
  - fixed the local coder template flow to use an explicit editor instead of a prompt-only save action

## [0.4.5] - 2026-03-10

### Added

- **Explicit workflow tool access controls in the control panel**:
  - added `All tools` vs `Custom allowlist` controls to the workflow creation wizard
  - added matching tool-access controls to the workflow automation edit modal
  - surfaced the selected tool policy in the wizard review step before deploy
  - wired workflow-plan apply to honor `tool_access_mode` and `tool_allowlist` through operator preferences

### Changed

- **Workflow automation editing and run-debug UX refinements**:
  - expanded the workflow edit modal into a large editor layout with a dedicated prompt-editing area for step objectives
  - made workflow provider/model selectors preserve saved values and use the panel’s styled selector treatment
  - improved the run debugger modal and workflow board sizing so the board can grow, the right rail no longer crops blocker/failure text, and lower log panels stop starving the board of height
  - tightened workflow prompt-editor cards by removing duplicated step text and redundant node-id badges
  - improved workflow review-step readability by collapsing long plan/prompt text into expandable markdown previews

### Fixed

- **Workflow automation schedule/tooling and publish reliability**:
  - fixed workflow automation save payloads to use the server-required tagged `misfire_policy` shape
  - fixed workflow agents defaulting to an overly narrow tool subset by making tool access explicit and configurable
  - fixed duplicate workflow-automation list rows in the control panel by normalizing rendered Automation V2 entries by id
  - corrected workflow debugger failure reporting for malformed shell tool calls and surfaced a clearer failure reason in the run debugger
  - hardened the engine loop so malformed tool calls such as `BASH_COMMAND_MISSING`, `WEBFETCH_URL_MISSING`, and missing file/write args get bounded inline self-repair retries before burning workflow node attempts
  - fixed `@frumu/tandem-client` publish builds by restoring missing `AgentStandupCompose*` type imports in the TypeScript client

## [0.4.4] - 2026-03-09

### Added

- **Official headless bootstrap path through `@frumu/tandem-panel`**:
  - added a real `tandem-setup` CLI with explicit `init`, `doctor`, `service`, `pair mobile`, and `run` commands
  - added shared bootstrap modules for canonical env-path resolution, env generation, engine-config bootstrapping, and install-time diagnostics
  - added cross-platform service generation for Linux `systemd` and macOS `launchd`
  - added a shared `service-runner` entrypoint so managed services use the same env loading and runtime startup contract on both platforms
  - added focused regression coverage for bootstrap env generation, `systemd` unit generation, `launchd` plist generation, and `doctor` output
- **Agent standups, reusable agent personalities, and workspace memory defaults**:
  - added reusable agent personalities in the control panel with persistent prompts, default models, and avatar upload
  - added server-side standup workflow composition on top of Automation V2 using saved agent personalities
  - added workspace-aware memory defaults so chats and automations can use `memory_search`, `memory_store`, and `memory_list` without manually supplying `session_id` or `project_id`
  - added deterministic `project_id` binding for workspace-backed sessions to improve recall across prior conversations in the same workspace
  - updated standup workflows to combine memory recall with workspace inspection through `glob`, `grep`, and `read`

### Changed

- **Control-panel bootstrap and runtime wiring**:
  - made `tandem-setup init` the documented bootstrap path while keeping legacy `tandem-control-panel --init` compatibility
  - updated control-panel bootstrap to prefer canonical OS config/data locations for official installs instead of cwd-only `.env` ownership
  - added `TANDEM_CONTROL_PANEL_HOST`, `TANDEM_CONTROL_PANEL_PUBLIC_URL`, and canonical control-panel state-dir handling for future gateway/mobile pairing work
  - updated the control-panel runtime to bind explicitly to the configured panel host and to load managed env files before startup
  - updated package/docs/example guidance to point headless installs at the control-panel gateway layer instead of the old quickstart bootstrap path

### Fixed

- **Automation V2 save verification after storage migration**:
  - fixed workflow-plan apply and control-panel automation saves failing with `WORKFLOW_PLAN_APPLY_FAILED` when stale legacy `automations_v2.json` files were still present alongside the canonical data file
  - kept strict persistence verification on the active canonical automation file while downgrading mismatched legacy fallback files to warnings instead of rejecting the save
  - added a regression test covering successful automation save/apply with a stale legacy `automations_v2.json` migration artifact present

## [0.4.3] - 2026-03-08

### Fixed

- **Automation V2 restart persistence and state-path hardening**:
  - fixed an engine startup race where the Automation V2 scheduler could persist an empty in-memory map before saved automations were loaded, wiping `automations_v2.json` on restart
  - moved Automation V2 canonical persistence into the Tandem global `data/` directory and kept legacy root-level files as migration fallback during load
  - added persistence verification, startup/load diagnostics, and recovery from run snapshots when automation definitions are missing but run history still exists

### Added

- **Tandem TUI coding-agent workflow UX**:
  - added coding-focused shortcuts:
    - `Alt+P` opens workspace file search and inserts `@path` references into the active composer
    - `Alt+D` opens a scrollable git diff overlay for quick workspace-change review
    - `Alt+E` opens the active composer content in `$VISUAL` / `$EDITOR` and returns edited text back into the TUI
  - added matching slash commands:
    - `/files [query]`
    - `/diff`
    - `/edit`
  - added modal infrastructure for coding workflows:
    - file-search modal with keyboard navigation/confirm
    - pager overlay with up/down/page scrolling
  - improved tool-call/tool-result transcript rendering into multi-line execution cells for better readability during coding runs
  - updated Tandem TUI docs and help content to surface the new coding workflow shortcuts and commands

- **Desktop orchestrator and command-center stability improvements**:
  - fixed orchestrator resume behavior so paused/failed runs with no planned tasks trigger planning (`start`) instead of attempting to execute an empty plan
  - improved run listing compatibility by merging engine context runs with legacy local orchestrator runs, preventing sessions from disappearing in mixed-storage scenarios
  - improved run deletion for engine context runs by removing shared `data/context_runs/<run_id>` state and surfacing real filesystem errors instead of silently ignoring failed deletes
  - replaced native desktop confirm popups in orchestrator flow with in-app confirmation dialogs for `Start Fresh Run` and resume model-switch actions
  - added in-app toast handling for provider quota/payment failures so users see actionable `payment required` / credit-limit errors in the UI
  - updated planner guidance to avoid over-collapsing non-trivial report/objective requests into a single task, restoring multi-task decomposition behavior
  - reduced `tool.lifecycle.start` log spam by suppressing duplicate in-flight `ToolStart` events for the same logical tool call even when providers stream arg updates
  - fixed command center run action visibility to rely on selected run identity directly, preventing cases where a selected running run looked stuck with no available controls
  - fixed validator/retry mismatch where write-intended tasks classified as `Research`/`Inspection`/`Validation` could bypass strict-write enforcement and loop into `Max retries exceeded` with `no changed-file evidence`; retries now auto-escalate to strict-write when validator feedback proves no workspace changes

- **Initial Tandem Coder engine API foundation**:
  - added engine-owned coder workflow runtime control through:
    - `GET /coder/status`
  - hardened coder memory promotion rules for newer memory kinds:
    - `duplicate_linkage` now requires both linked issue and linked pull-request numbers before promotion
    - `regression_signal` now requires structured regression entries plus supporting review/summary evidence
    - generic terminal `run_outcome` backfills can no longer be promoted without workflow evidence artifacts
  - broadened duplicate-linkage memory beyond PR submit so PR review and merge follow-on runs now persist their own issue↔PR linkage candidates from parent issue-fix runs
  - broadened post-failure regression learning beyond Bug Monitor by writing `regression_signal` candidates when issue-triage reproduction fails
  - issue-fix validation failures now also emit `regression_signal` memory with failed validation evidence
  - issue-fix worker-session failures now also emit rich `run_outcome` memory instead of relying only on generic terminal backfill
  - triage, review, and merge worker-session failures now also fail runs truthfully and emit rich `run_outcome` memory with worker artifact/session context
  - issue-fix retrieval now prioritizes `regression_signal` memory so failed validation history can influence later fixes across related issues
    - `GET /coder/projects/{project_id}`
    - `GET /coder/projects/{project_id}/runs`
    - `POST /coder/projects/{project_id}/runs`
    - `GET /coder/projects/{project_id}/bindings`
    - `PUT /coder/projects/{project_id}/bindings`
    - `POST /coder/runs/{id}/execute-next`
  - `GET /coder/status` now summarizes total runs, active/awaiting-approval counts, workflow distribution, run-status distribution, project count, and the latest coder run directly from engine-owned run state
  - added `GET /coder/projects`, which summarizes known repo bindings, workflow coverage, latest run metadata, and project-level coder policy from existing engine-owned run state
  - added `GET /coder/projects/{project_id}`, which returns project policy, explicit binding, and recent run state in one engine-backed payload
  - added `GET /coder/projects/{project_id}/runs`, which returns project-scoped coder runs with execution policy and merge policy summaries already attached
  - added `POST /coder/projects/{project_id}/runs`, which creates coder runs from a saved project binding and fails closed with `CODER_PROJECT_BINDING_REQUIRED` when that binding has not been configured
  - added an explicit shared coder memory retrieval helper and surfaced its `retrieval_policy` on run detail and `GET /coder/runs/{id}/memory-hits`, so workflow-specific ranking is now part of the engine contract instead of an implicit triage-only implementation detail
  - `issue_triage` retrieval now prioritizes `regression_signal` alongside `failure_pattern`, and promoted triage reproduction failures can now be reused across related issues through governed memory because regression-signal promotion accepts reproduction and validation evidence artifacts in addition to summary/review artifacts
  - `issue_triage` can now infer duplicate pull-request candidates from historical `duplicate_linkage` memory and writes its own `duplicate_linkage` candidate when triage concludes an issue is already covered by linked PR history
  - triage/fix retrieval now gives `duplicate_linkage` more weight, so cross-workflow issue↔PR history surfaces ahead of more generic triage memory when linked duplicates exist
  - `pr_review` now reuses prior `merge_recommendation_memory` on the same PR, and `merge_recommendation` now reuses prior `review_memory` on the same PR, so adjacent workflow context is available through the shared retrieval layer instead of depending only on governed-memory fallback
  - real issue-fix PR submit now writes `duplicate_linkage` memory that links issue and pull-request numbers, returns that candidate in submit responses/events/artifacts, and makes it reusable in follow-on PR review retrieval
  - generic terminal coder transitions now backfill a reusable `run_outcome` candidate for failed and cancelled runs when no richer workflow-specific outcome already exists, and return that generated candidate directly from the transition response
  - Bug Monitor triage summaries now also persist governed `regression_signal` memory alongside `failure_pattern`, with a matching context-run artifact and structured expected-behavior context for later post-failure reuse
  - explicit project bindings can now be stored independently of runs, and `/coder/projects` now prefers those saved bindings over derived run bindings when both exist
  - merge-ready `merge_recommendation` runs now stop in `awaiting_approval`, emit `coder.approval.required`, and complete cleanly on `/coder/runs/{id}/approve` instead of bouncing back into `running`
  - approving a merge-ready recommendation now also writes an engine-owned `coder_merge_execution_request` artifact and emits `coder.merge.recommended`, so the post-approval merge handoff is explicit even before a real GitHub merge capability is wired
  - added `POST /coder/runs/{id}/merge-submit` with fail-closed `github.merge_pull_request` readiness, a persisted `coder_merge_submission` artifact, and a real MCP-backed merge path for approved merge recommendations
  - `merge-submit` now also fails closed on the approved handoff artifact itself, blocking execution unless the latest `coder_merge_execution_request` still says `recommendation = merge` with no remaining blockers, checks, or approvals
  - `merge-submit` now defaults to `submit_mode = manual` and rejects `submit_mode = auto` unless the follow-on run origin policy explicitly opted into auto merge execution
  - `merge-submit` now also requires an approving sibling `pr_review` for issue-fix follow-on merge runs, so a completed review with blockers or requested changes can no longer be used to push a merge through
  - `merge-submit` now evaluates the latest completed sibling `pr_review`, so a newer review with requested changes overrides an older approval instead of whichever review record happens to be discovered first
  - merge-ready recommendation approvals and merge run reads now expose a dynamic `merge_submit_policy` summary, so clients can see manual vs auto merge-submit eligibility before attempting the merge call
  - `coder_merge_execution_request` artifacts and `coder.merge.recommended` events now also carry a `merge_submit_policy_preview`, so streaming and artifact-driven clients get the same merge-submit policy context without a follow-up read
  - `merge-submit` now requires the merge run itself to be an auto-spawned follow-on before `submit_mode = auto` is eligible, so a manual follow-on merge run cannot escalate into auto merge execution even if the parent PR submit opted into auto merge recommendation
  - `merge_submit_policy` summaries now also include `preferred_submit_mode`, so clients and future automation can consume an engine-owned recommendation instead of inferring manual-vs-auto behavior from blocked flags alone
  - `merge_submit_policy` summaries now also make the current execution contract explicit with `explicit_submit_required = true` and `auto_execute_after_approval = false`, so clients know approval alone never auto-merges today
  - `merge_submit_policy` summaries now also include `auto_execute_eligible` and `auto_execute_block_reason`, so clients can distinguish “auto is preferred later” from “auto can run now” without reverse-engineering that from other flags
  - added `GET /coder/projects/{project_id}/policy` and `PUT /coder/projects/{project_id}/policy`, with a default-off project-level `auto_merge_enabled` switch that now feeds `merge_submit_policy.auto_execute_policy_enabled` and changes merge-ready auto-execution blocking to `project_auto_merge_policy_disabled` until a project explicitly opts in
  - `merge_submit_policy.auto_execute_eligible` now becomes `true` when a merge run is auto-spawned, review-approved, merge-ready, and the project-level `auto_merge_enabled` switch is on, while still keeping `explicit_submit_required = true` and `auto_execute_after_approval = false` so the engine tells the truth about readiness without auto-merging yet
  - `POST /coder/runs` now also returns `merge_submit_policy` for merge-recommendation runs, so manual and spawned merge follow-on creation responses surface project auto-merge policy and merge-submit prerequisites immediately instead of forcing a follow-up run read
  - `merge_submit_policy.auto_execute_block_reason` now reports the earliest real blocker (`requires_merge_execution_request`, `requires_completed_pr_review_follow_on`, `requires_approved_pr_review_follow_on`, etc.) instead of collapsing those states back to a generic `preferred_submit_mode_manual`
  - PR-submit `follow_on_runs` templates now also carry `merge_submit_policy_preview` for merge follow-ons, so clients can see project auto-merge policy and merge-submit prerequisites before the merge run even exists
  - merge-ready `coder.approval.required` events and `merge-recommendation-summary` responses now also carry `merge_submit_policy`, so streaming clients can see merge-submit readiness and project auto-merge policy without fetching the run
    - `POST /coder/runs/{id}/execute-all`
  - added structured intermediate and final artifacts for triage inspection/reproduction, issue-fix validation and patch evidence, PR review evidence, and merge readiness
  - added governed-memory-aware retrieval and reusable coder memory outputs across `issue_triage`, `issue_fix`, `pr_review`, and `merge_recommendation`
  - added engine-owned issue-fix PR drafting and approval-gated submit handoff through:
    - `POST /coder/runs/{id}/pr-draft`
    - `POST /coder/runs/{id}/pr-submit`
  - normalized PR-submit artifacts now preserve stable repo context and a canonical `submitted_github_ref`, and PR-result parsing now accepts minimal number-only GitHub/MCP result shapes so later review and merge flows have a stable PR handoff target
  - fixed PR submit MCP tool resolution so builtin raw tool names and runtime namespaced tool names both resolve correctly, and added a real HTTP-backed MCP regression that exercises non-dry-run PR submission end to end
  - added server-side follow-on run creation at `POST /coder/runs/{id}/follow-on-run`, so a successful issue-fix PR submission can spawn `pr_review` or `merge_recommendation` runs from the canonical submitted PR ref without frontend-owned handoff logic
  - PR submit artifacts now also carry machine-readable `follow_on_runs` templates so later review/merge workflows can be chained from the engine-owned submission payload without reconstructing run inputs in the UI
  - `POST /coder/runs/{id}/pr-submit` now also returns `submitted_github_ref`, `pull_request`, and `follow_on_runs` directly in the API response so clients do not need a second artifact read to continue the workflow
  - `coder.pr.submitted` events now include the canonical submitted PR ref, PR number, and follow-on workflow templates so streaming clients can continue the workflow without a follow-up fetch
  - `POST /coder/runs/{id}/pr-submit` can now optionally auto-create follow-on `pr_review` and `merge_recommendation` runs through engine-owned chaining, returning those spawned runs directly in `spawned_follow_on_runs`
  - auto-follow-on merge chaining now normalizes through review first, so requesting `merge_recommendation` auto-spawn implicitly schedules `pr_review` ahead of merge instead of trusting the client to order those runs correctly
  - merge auto-follow-ons are now explicit opt-in at submit time: requesting merge auto-spawn without `allow_auto_merge_recommendation` only auto-spawns `pr_review`, records the skipped merge follow-on with a machine-readable reason, and includes both spawned and skipped follow-on lists in the submission artifact and `coder.pr.submitted` event payload
  - spawned and manual follow-on coder runs now persist explicit `parent_coder_run_id`, `origin`, and `origin_artifact_type` metadata so review and merge runs can be traced back to the issue-fix PR submission that created them
  - `pr_review` now uses the real coder worker-session bridge during `review_pull_request`, persists `coder_pr_review_worker_session`, and feeds the parsed worker output into the existing review-evidence and final summary artifact flow instead of synthesizing review text inline
  - `merge_recommendation` now uses the real coder worker-session bridge during `assess_merge_readiness`, persists `coder_merge_recommendation_worker_session`, and feeds parsed worker output into the existing readiness and final summary artifact flow instead of hardcoded merge guidance
  - `issue_triage` now uses the real coder worker-session bridge during repo inspection, persists `coder_issue_triage_worker_session`, and reuses parsed worker output for inspection, reproduction, and final summary artifacts instead of synthetic triage step payloads
  - follow-on review and merge runs now persist structured `origin_policy` metadata so downstream runs know whether they were manual vs auto-spawned and whether merge auto-spawn had been explicitly opted in at submit time
  - PR-submit `follow_on_runs` templates now carry the same parent/origin policy context that spawned review and merge runs use, so clients can preview engine chaining policy before creating any downstream runs
  - follow-on run `origin_policy` now consistently uses `merge_auto_spawn_opted_in` for both templates and spawned/manual runs, avoiding two names for the same merge auto-spawn decision
  - merge-recommendation follow-on runs created from PR submission are now execution-gated until a sibling `pr_review` run completes, so merge assessment cannot run ahead of review even when the follow-on run already exists
  - PR-submit follow-on templates and merge follow-on run metadata now expose `required_completed_workflow_modes`, so clients can surface the review-before-merge prerequisite without inferring policy locally
  - `GET /coder/runs` and `GET /coder/runs/{id}` now return engine-owned `execution_policy` summaries for coder runs, so clients can tell when a merge follow-on is blocked by review policy before attempting execution
  - PR-submit handoff payloads now also expose follow-on `execution_policy_preview` metadata and live `execution_policy` on spawned follow-on runs, so clients can render review-before-merge gating directly from the submit response, artifact, and event payloads without an extra run fetch
  - `POST /coder/runs` now also returns `execution_policy`, so manual follow-on creation responses are immediately truthful about blocked merge-recommendation runs without requiring a follow-up read
  - blocked `execute-next` / `execute-all` responses now also return `coder_run`, `run`, and `execution_policy`, so clients can stay in sync after a policy block without issuing a second fetch
  - blocked `execute-next` / `execute-all` now also emit `coder.run.phase_changed` with `event_type = execution_policy_blocked`, so streaming clients can react to follow-on policy blocks without polling
  - merge-recommendation summaries that come back `merge` with no remaining blockers/checks/approvals now stop in `awaiting_approval` and emit `coder.approval.required`, instead of looking fully completed before an operator approves the recommendation

- **Bug Monitor settings foundation and server config/status surface**:
  - added persisted bug-monitor config and status state in `tandem-server`, including repo, MCP server, provider preference, and dedicated `model_policy.default_model` routing for the reporter agent
  - added fail-closed readiness validation for the selected provider/model, required GitHub capabilities, and selected MCP server connectivity
  - fixed the control-panel Bug Monitor tab initialization crash caused by early query access
  - changed reporter model selection from a strict catalog-only dropdown to typed model entry with provider suggestions so manual model IDs persist across reloads
  - generalized GitHub MCP capability readiness so arbitrary MCP server instance names can satisfy reporter issue capabilities instead of depending on hardcoded provider-style server names
  - added new HTTP endpoints for reporter configuration and operator visibility:
    - `GET /config/bug-monitor`
    - `PATCH /config/bug-monitor`
    - `GET /bug-monitor/status`
    - `GET /bug-monitor/drafts`
    - `GET /bug-monitor/drafts/{id}`
  - control-panel Settings now includes a dedicated `Bug Monitor` tab with:
    - enable/disable toggle
    - target repo configuration
    - reuse of existing MCP server connections
    - dedicated provider/model selection for a cheap/fast reporter route
    - readiness, capability coverage, and draft summary cards
  - added a direct `#/bug-monitor` control-panel route as the canonical Settings entry for Bug Monitor
  - desktop Settings now includes an engine-backed `Bug Monitor` surface with:
    - repo, MCP server, provider, and model route configuration
    - runtime readiness and capability visibility
    - recent draft visibility
    - deep-link into `Extensions -> MCP`
  - added a Tauri bridge for Bug Monitor config, status, draft listing, draft lookup, and manual report submission
  - fixed the desktop sidecar Bug Monitor config path to use the engine’s canonical `GET/PATCH /config/bug-monitor` route instead of a non-existent `/bug-monitor/config` path
  - added `POST /bug-monitor/report` so desktop and future clients can submit structured failure context through the engine and receive a deduped local draft record
  - desktop logs and failed orchestrator runs can now create Bug Monitor drafts directly without implementing issue logic in the frontend
  - added engine-owned Bug Monitor draft approval endpoints:
    - `POST /bug-monitor/drafts/{id}/approve`
    - `POST /bug-monitor/drafts/{id}/deny`
  - desktop Settings can now approve or deny `approval_required` Bug Monitor drafts directly from the recent-drafts list
  - control-panel Settings now uses the same engine draft approval endpoints so Bug Monitor draft decisions work consistently across both UIs
  - added engine-backed triage-run creation for approved Bug Monitor drafts through:
    - `POST /bug-monitor/drafts/{id}/triage-run`
  - triage-run creation now seeds a minimal `bug_monitor_triage` context run with inspection and validation blackboard tasks, plus draft-to-run dedupe through `triage_run_id`
  - desktop and control-panel Settings can now promote approved Bug Monitor drafts into triage context runs without frontend-owned run orchestration
  - control-panel Dashboard context-run visibility now includes `bug_monitor_triage` runs in the existing context-run drawer so the handoff can be inspected without a new page
  - added `POST /bug-monitor/drafts/{id}/issue-draft`, which renders a template-aware Bug Monitor issue draft artifact from the repo bug template before GitHub publish
  - Bug Monitor GitHub publish now uses that rendered issue-draft artifact instead of dumping raw `draft.detail` into new issues
  - auto-publish now defers with `triage_pending` until a triage-backed issue draft exists, preventing premature low-signal issue creation
  - fixed Bug Monitor incident persistence so draft-creation failures record a visible incident error instead of leaving a half-created incident with no draft or explanation
  - approving a Bug Monitor draft no longer fails the operator action just because the follow-up GitHub publish step is blocked
  - split Bug Monitor readiness into local ingest vs GitHub publish readiness so live tracker surfaces can report “watching locally” instead of incorrectly showing the monitor as fully blocked
  - added `POST /bug-monitor/drafts/{id}/triage-summary` so Bug Monitor triage can write a structured summary artifact with `what_happened`, `expected_behavior`, `steps_to_reproduce`, `environment`, and `logs`
  - Bug Monitor issue-draft generation now prefers that structured triage summary artifact over raw incident detail when rendering the repo issue template
  - Bug Monitor now suppresses duplicate incidents earlier in both runtime ingest and manual `POST /bug-monitor/report` flows by consulting stored `failure_pattern` memory before opening a fresh draft
  - Bug Monitor incidents now persist a compact duplicate summary when suppression happens so tracker UIs can explain duplicate suppression after reload/reconnect without overloading the raw source-event payload
  - Bug Monitor triage summaries now persist governed `failure_pattern` memory for subject `bug_monitor`, so structured triage can suppress later matching reports even without a prior coder-run artifact
  - Approving a Bug Monitor draft without triage now also persists governed `failure_pattern` memory from the approved draft itself, so operator-approved issues still teach duplicate suppression
  - `failure_pattern` memory now carries recurrence metadata and stronger issue-linkage metadata, and duplicate ranking uses recurrence as a tie-breaker after exact fingerprint matches
  - duplicate-suppressed Bug Monitor incidents now persist a normalized `duplicate_summary` envelope with match count, best-match details, recurrence metadata, and linked-issue unions so tracker UIs can explain suppression deterministically after reload/reconnect
  - manual `POST /bug-monitor/report` suppression now returns that same normalized `duplicate_summary` envelope, and failure-pattern matching reuses the exact-fingerprint -> recurrence -> score ordering so the reported best match stays aligned with runtime suppression
  - Bug Monitor failure-pattern reuse responses now attach the same normalized `duplicate_summary` envelope alongside any raw `duplicate_matches`, and coder-originated duplicate matches now emit a stable `match_reason` so exact-fingerprint priority survives through shared ranking and summary shaping

- **Initial Tandem Coder engine API foundation**:
  - added a first engine-owned coder API surface:
    - `POST /coder/runs`
    - `GET /coder/runs`
    - `GET /coder/runs/{id}`
    - `GET /coder/runs/{id}/artifacts`
    - `POST /coder/runs/{id}/execute-next`
  - coder runs are persisted as thin metadata records linked to engine context runs instead of a frontend-owned workflow store
  - `POST /coder/runs` now creates a linked `coder_issue_triage` context run for `issue_triage` workflows and seeds the first deterministic task template:
    - normalize issue reference
    - retrieve prior memory
    - inspect repo
    - attempt reproduction
    - write triage artifact
  - added initial `coder.run.created` event emission so clients can observe coder run creation through the engine event bus
  - `issue_triage` now has a first real worker bridge: `execute-next` claims the next runnable context task through the shared lease/claim runtime and dispatches deterministic inspection, reproduction, and final summary actions so the run can complete end to end without frontend-owned orchestration
  - `issue_fix` now uses that same `execute-next` worker bridge: the engine claims fix tasks through the shared task runtime, advances inspection and preparation nodes through workflow progression, and dispatches validation plus final summary handlers to complete the run end to end
  - `pr_review` now also uses `execute-next`: the engine claims review tasks through the same task runtime, advances the initial inspection node through workflow progression, and dispatches review-evidence plus final summary handlers to complete the run end to end
  - `merge_recommendation` now uses `execute-next` too: the engine claims merge-readiness tasks through the same task runtime, advances the initial inspection node through workflow progression, and dispatches readiness plus final recommendation handlers to complete the run end to end
  - added `POST /coder/runs/{id}/execute-all`, which loops that same engine-owned task runtime until a coder run completes, fails, cancels, exhausts runnable tasks, or hits a configured step cap
  - added backend regression coverage for coder run creation, retrieval, list projection, context-run linkage, and artifact projection
  - added the first fail-closed readiness gate for `issue_triage` coder runs:
    - required GitHub issue capability bindings must exist
    - explicitly requested MCP servers must be configured and connected
  - added `POST /coder/runs/{id}/memory-candidates` so `issue_triage` runs can persist engine-owned memory candidate payloads with blackboard artifact provenance
  - memory candidate writes now emit `coder.memory.candidate_added` and attach `coder_memory_candidate` artifacts to the linked context run
  - new `issue_triage` runs now seed their retrieval task with prior repo/issue memory candidate hints from earlier coder runs
  - added `POST /coder/runs/{id}/triage-summary` so the engine can write a concrete `triage.summary.json` artifact and attach it as `coder_triage_summary`
  - added `GET /coder/runs/{id}/memory-hits` so clients can inspect ranked triage retrieval hits for the current coder run
  - `issue_triage` bootstrap now combines prior `coder_memory_candidate` payloads with project semantic memory search and writes a `coder_memory_hits` artifact into the linked context run
  - triage summary writes now auto-generate reusable `triage_memory` and `run_outcome` memory candidates so later coder runs can reuse structured triage conclusions without a second manual write step
  - `issue_triage` memory retrieval now also ranks governed/shared memory hits from the existing engine memory database alongside project semantic memory and prior coder-local candidates
  - added `POST /coder/runs/{id}/memory-candidates/{candidate_id}/promote` so reviewed coder memory candidates can be stored in the governed memory database and optionally marked shared when reviewer and approval metadata are supplied
  - added thin coder control endpoints:
    - `POST /coder/runs/{id}/approve`
    - `POST /coder/runs/{id}/cancel`
  - coder control endpoints now replay the existing context-run `plan_approved` and `run_cancelled` transitions instead of introducing a second coder lifecycle model
  - added `coder.run.phase_changed` emission for coder approve/cancel transitions and fixed cancelled coder runs to project a dedicated `cancelled` phase
  - coder `issue_triage` readiness now reuses the shared engine capability-readiness evaluator, so run creation blocks on the same missing/unbound/disconnected/auth-pending conditions exposed by `/capabilities/readiness`
  - explicit `mcp_servers` requested by coder runs remain hard requirements on top of that shared readiness check
  - the HTTP test harness now seeds a connected GitHub MCP server/tool cache so coder tests exercise the real readiness/discovery path rather than a reduced fallback
  - unified coder memory promotion with the generic governed-memory contract: the coder adapter now reuses the shared `memory_put` / `memory_promote` implementation path instead of writing directly to the global memory database
  - factored run-scoped governed-memory capability issuance into shared helpers in `skills_memory.rs`, so coder workflows now derive their subject and tier policy through the same helper path as the generic memory routes instead of hand-building a parallel token shape
  - fixed cold-start global memory initialization so `/memory/*` routes create the memory DB parent directory before opening SQLite, which also keeps the shared governed-memory path reliable for coder promotion
  - normalized coder event payloads so `coder.run.created`, `coder.run.phase_changed`, `coder.artifact.added`, `coder.memory.candidate_added`, and `coder.memory.promoted` now share the same base run metadata and artifact events carry explicit `kind` context for consumers
  - added `POST /coder/runs/{id}/pr-review-summary` so `pr_review` runs can write a structured `coder_pr_review_summary` artifact and emit a first `run_outcome` memory candidate
  - added a first `pr_review` coder workflow skeleton on top of context runs, including fail-closed GitHub pull-request readiness checks, seeded PR review tasks, and direct MCP GitHub capability bindings for pull-request list/get/comment actions
  - `pr_review` now defaults to pull-request-specific memory queries, bootstraps a `coder_memory_hits` artifact at run creation, and reuses prior review `run_outcome` memory during later reviews of the same repo/PR
  - `pr_review` summary writes now also emit reusable `review_memory` candidates, and follow-on PR reviews can retrieve that review-specific memory through the shared coder memory-hits path
  - `pr_review` summary writes now also emit `regression_signal` candidates when review input includes historical regression signals, and later PR reviews can retrieve those signals through the same repo/PR memory-hits path
  - added the first `merge_recommendation` coder workflow skeleton with PR-backed readiness checks, seeded merge-assessment tasks, bootstrapped `coder_memory_hits`, and `POST /coder/runs/{id}/merge-recommendation-summary` for structured merge recommendation artifacts
  - merge recommendation summaries now emit reusable `merge_recommendation_memory` and `run_outcome` candidates so later runs can reuse prior merge guidance without needing a separate manual candidate write
  - added a dedicated `merge_recommendation_memory` candidate kind so merge guidance is stored as reusable recommendation knowledge instead of only a generic run outcome
  - added the first `issue_fix` coder workflow skeleton with issue-backed readiness checks, seeded fix and validation tasks, bootstrapped `coder_memory_hits`, and `POST /coder/runs/{id}/issue-fix-summary` for structured fix summary artifacts with reusable `run_outcome` memory
  - `issue_fix` summary writes now also emit reusable `fix_pattern` memory so later fix runs can reuse prior patch strategies and validation context
  - `issue_fix` memory retrieval now ranks same-issue `fix_pattern` and issue-fix `run_outcome` hits ahead of generic triage memory so fix runs surface prior patch strategy first
  - `merge_recommendation` memory retrieval now ranks same-PR `merge_recommendation_memory`, merge run outcomes, and regression signals ahead of generic review memory so merge runs surface prior merge guidance first
  - `merge_recommendation` ranking now also prefers policy-rich same-PR memories that carry blockers, required checks, or required approvals over generic merge summaries, so readiness-specific history surfaces first
  - `pr_review` memory retrieval now ranks same-PR `review_memory`, `regression_signal`, and PR review outcomes ahead of generic triage memory so review runs surface prior review guidance first
  - `pr_review` ranking now also prefers richer same-PR review memories that carry blockers, requested changes, or regression signals over generic review summaries, so actionable review history surfaces first
  - `issue_triage` memory retrieval now ranks same-issue `failure_pattern`, triage-memory, and triage run outcomes ahead of generic hits so duplicate/root-cause history surfaces first during new triage runs
  - `issue_fix` summary writes now also emit a dedicated `coder_validation_report` artifact when validation steps or results are provided, so validation evidence is consumable without parsing the fix summary
  - `issue_fix` summary writes now also emit reusable `validation_memory` candidates, and same-issue fix retrieval ranks that validation-specific memory ahead of generic triage memory so later fix runs can reuse prior validation evidence directly
  - `issue_fix` now starts using a real coder-owned worker session during `prepare_fix`: the engine resolves a worker model, creates a scoped session in the repo workspace, runs a real prompt through `run_prompt_async_with_context`, and persists the worker transcript as a `coder_issue_fix_worker_session` artifact before validation continues
  - `prepare_fix` now also derives a deterministic `coder_issue_fix_plan` artifact from the worker transcript, so downstream validation and final summary steps reuse a stable fix-plan record instead of dropping the worker output on the floor
  - `validate_fix` now also launches a real coder-owned validation session and persists a `coder_issue_fix_validation_session` artifact, so validation evidence is coming from the same engine session/runtime path instead of a synthetic placeholder step
  - `prepare_fix` now also harvests concrete changed-file evidence from worker tool invocations and persists it as `coder_changed_file_evidence`, giving later fix validation and UI surfaces an engine-owned record of touched paths when the worker actually edits files
  - changed-file evidence now captures per-file tool provenance plus short content previews when worker tool args include editable payloads, and final `coder_patch_summary` artifacts now carry those harvested entries forward for downstream review surfaces
  - `issue_fix` patch evidence now also snapshots the touched workspace files from the engine side, attaching lightweight file-existence, size, line-count, and preview metadata to both `coder_changed_file_evidence` and `coder_patch_summary`
  - final issue-fix summaries now also emit a dedicated `coder_patch_summary` artifact that ties the structured fix summary to changed files plus the linked worker and validation session IDs, giving desktop and future UIs a stable engine-owned patch-summary surface before full diff harvesting is added
  - added `POST /coder/runs/{id}/pr-draft` for `issue_fix`, which builds an engine-owned `coder_pr_draft` artifact from the latest fix summary, validation, and patch evidence and emits `coder.approval.required` for human review before submission
  - added `POST /coder/runs/{id}/pr-submit`, which reuses that `coder_pr_draft`, enforces fail-closed `github.create_pull_request` readiness, and writes a `coder_pr_submission` artifact for dry-run or approved submission flows
  - `issue_fix` validation and final summary generation now reuse those worker-session, validation-session, and issue-fix-plan artifacts, attaching session IDs, transcript excerpts, and plan-derived fields instead of relying only on generic inline placeholders
  - fixed a small set of ownership bugs in `skills_memory.rs` that were blocking `tandem-server` validation for the shared governed-memory path used by coder promotion and worker-backed issue-fix execution
  - `issue_triage` memory retrieval now ranks same-issue `failure_pattern`, `triage_memory`, and issue-triage `run_outcome` hits above generic project/governed matches so triage runs surface prior failure signatures and conclusions first
  - repo-scoped coder memory retrieval is now GitHub-ref-aware, so `pr_review` and `merge_recommendation` get a true same-PR boost instead of only issue-number or recency bias
  - promoted coder memory now stores richer searchable governed-memory content from workflow payloads, including fix strategy, root cause, blockers, required checks, approvals, validation details, and regression summaries instead of only a bare summary string
  - merge recommendation summaries now also write a dedicated `coder_merge_readiness_report` artifact whenever blockers, required checks, or required approvals are present so merge readiness state can be consumed directly without reparsing the summary artifact

- **Setup-understanding across channels and chat surfaces**:
  - added a shared deterministic setup-intent resolver at `POST /setup/understand` in `tandem-server`
  - setup understanding now classifies provider setup, MCP/integration setup, automation creation, channel setup help, broad setup help, and normal pass-through chat
  - resolver scoring now uses explicit setup/workflow language, provider/model and integration entity matches, MCP catalog presence, and current state gaps such as missing providers or disconnected integrations
  - added backend regression coverage for provider setup interception, integration interception, automation interception, clarification of broad setup requests, and normal-chat pass-through

- **Channel setup interception and scoped clarification**:
  - `tandem-channels` now calls the shared setup-understanding endpoint before normal prompt routing
  - clear automation setup requests in Telegram, Discord, and Slack now launch Pack Builder preview directly instead of relying on the brittle pack-intent matcher alone
  - provider and integration setup requests in channels now return deterministic setup guidance instead of spending a full LLM turn on ordinary chat
  - ambiguous setup requests now create a scoped pending clarifier so the next reply resolves within the same channel conversation/thread/topic
  - existing slash commands and Pack Builder confirm/cancel reply handling remain the highest-priority deterministic paths

- **Desktop and control-panel chat setup cards**:
  - desktop chat now preflights outgoing messages through setup understanding and renders setup cards that route users into Settings, MCP/Extensions, or Pack Builder preview
  - control-panel chat now performs the same setup preflight and surfaces setup cards that route users into Settings, MCP, or Automations
  - added a Tauri sidecar bridge for setup understanding so desktop chat uses the same backend interpretation contract as channels and the control panel

## [0.4.1] - 2026-03-07

### Added

- **Scoped channel sessions and trigger-aware channel routing foundations**:
  - `tandem-channels` now attaches structured trigger metadata (`direct_message`, `mention`, `reply_to_bot`, `ambient`) and stable conversation scope metadata (`direct`, `room`, `thread`, `topic`) to incoming channel messages
  - channel session identity now scopes by conversation, not just sender, so the same user no longer shares one active session across unrelated Discord channels/threads, Slack threads, and Telegram private/topic contexts
  - dispatcher slash-command/session resolution now uses the scoped conversation key and migrates legacy `{channel}:{sender}` channel-session records forward on first access
  - Slack now supports `mention_only` end-to-end across runtime env config, persisted server config, and `/channels/config`
  - adapter normalization/gating was tightened so Telegram, Discord, and Slack all make mention-only decisions from explicit trigger context rather than adapter-local string stripping
  - added regression coverage for scoped key generation, legacy session-map migration, Slack mention normalization, and channel config surface behavior

- **Strict swarm write recovery and engine-client retry hardening**:
  - fixed OpenAI/OpenRouter streamed tool-call parsing so multi-chunk write args keep the real tool-call ID and no longer drop follow-up argument deltas when later chunks omit the tool name
  - hardened write-arg recovery in the engine loop so truncated/malformed JSON can still recover `content` even when `path` is absent, and raised the default provider output budget from `2048` to `16384` to avoid clipping large single-file artifacts
  - session/tool history persistence now keeps write args/results intact across engine events by using a dedicated session-part channel, coalescing repeated tool invocation updates, and persisting `tool_result` args for verifier/session consumers
  - swarm planner/worker prompts now prefer single-pass implementation for single-file objectives instead of over-decomposing greenfield artifact creation into fragile inspect/refine task chains
  - added consistent local-engine retry handling across control panel, Tauri desktop, and TUI clients for transient transport failures and `ENGINE_STARTING` startup responses

- **Orchestration planner/runtime diagnostics hardened**:
  - strict swarm planning now surfaces upstream provider failures directly in orchestration start instead of collapsing them into `LLM planner returned no valid tasks`
  - backend session dispatch now persists explicit assistant-visible engine error markers such as `ENGINE_ERROR: AUTHENTICATION_ERROR: ...` so planner/session consumers can show the real provider failure reason
  - control-panel planner detection now recognizes provider quota/auth failures (for example OpenRouter `403 Key limit exceeded`) and reports them clearly when LLM planning is required
  - session tool-history persistence now recognizes the real runtime `WireMessagePart` shape used by backend tool invocation/result events, fixing false `NO_TOOL_ACTIVITY_NO_WORKSPACE_CHANGE` swarm task failures when tools actually ran
  - added regression coverage for runtime-shaped tool-part persistence in session history so verifier/session snapshots stay aligned with backend execution

- **Provider catalog honesty in Settings and `/provider`**:
  - `GET /provider` now returns explicit provider catalog metadata (`catalog_source`, `catalog_status`, `catalog_message`) so clients can distinguish live remote catalogs from config-defined or unavailable catalogs
  - removed synthetic single-model fallback catalog entries for built-in providers from the provider catalog response
  - provider catalog discovery now attempts live remote model lists for supported providers (`openrouter`, `openai`, `groq`, `mistral`, `together`, `anthropic`, `cohere`) and reports manual-entry states for providers without reliable generic discovery
  - remote provider model discovery now reads runtime-auth and persisted provider auth stores in addition to config/env values, so authenticated catalogs can load honestly in Settings without forcing API keys into config files
  - control-panel Settings now labels provider catalogs accurately:
    - real remote counts for live catalogs
    - `configured models` for config-defined catalogs
    - `manual entry` for providers without live model discovery
  - unsupported/non-generic providers remain configurable in Settings without being misrepresented as one-model catalogs
  - added backend regression coverage for empty/manual providers and config-sourced provider catalogs

- **Declarative workflow runtime, APIs, and pack extensions**:
  - added a new `tandem-workflows` workspace crate for workflow schema definitions, YAML loading, source merge rules, and validation
  - added engine-owned workflow runtime execution, hook dispatch, simulation, run persistence, and event streaming in `tandem-server`
  - added workflow HTTP APIs:
    - `GET /workflows`
    - `GET /workflows/{id}`
    - `POST /workflows/validate`
    - `POST /workflows/simulate`
    - `POST /workflows/{id}/run`
    - `GET /workflows/runs`
    - `GET /workflows/runs/{id}`
    - `GET /workflow-hooks`
    - `PATCH /workflow-hooks/{id}`
    - `GET /workflows/events`
  - pack inspection now surfaces declared workflow entrypoints, workflow files, and workflow hooks as first-class pack metadata and risk/permission-sheet signals
  - added workflow runtime coverage for manual runs, hook dispatch/dedupe behavior, context-run blackboard projection, and provider/catalog-backed route handling

- **Control-panel visual system and workflow operations refresh**:
  - introduced a shared `tandem-theme-contract` package plus a richer control-panel theme system with curated theme metadata, CSS variables, and a dedicated theme picker surface
  - added reusable animated UI primitives and refreshed page shells/cards/layouts across dashboard, orchestrator, packs, chat, feed, settings, and login flows
  - packs/workflows operations now have a dedicated control-panel workflow lab for pack inspection, workflow runs, hook toggles, event simulation, and live workflow event streaming
  - orchestration/swarm server routes now track per-run controller state more explicitly for status, revision requests, and run switching instead of relying on a single global active-run snapshot

- **Headless-first Chromium browser automation with readiness diagnostics**:
  - added a new `tandem-browser` Rust sidecar crate for local Chromium automation over stdio with typed browser RPC methods (`browser.open`, `browser.navigate`, `browser.snapshot`, `browser.click`, `browser.type`, `browser.press`, `browser.wait`, `browser.extract`, `browser.screenshot`, `browser.close`)
  - added browser readiness and install diagnostics with real launch smoke tests, distro-aware Linux install hints, and sidecar/browser detection for headless VPS hosts and desktops
  - added engine-managed sidecar install and discovery:
    - `tandem-engine browser install`
    - `POST /browser/install`
    - managed sidecar resolution from Tandem shared `binaries/` storage so normal installs do not require `TANDEM_BROWSER_SIDECAR`
    - release packaging/upload for standalone `tandem-browser-*` GitHub release assets
  - added browser runtime config and env support:
    - `browser.enabled`
    - `browser.sidecar_path`
    - `browser.executable_path`
    - `browser.headless_default`
    - `browser.allow_no_sandbox`
    - `browser.user_data_root`
    - `browser.max_sessions`
    - `browser.default_viewport`
    - `browser.allowed_hosts`
    - `TANDEM_BROWSER_SIDECAR`
    - `TANDEM_BROWSER_EXECUTABLE`
    - `TANDEM_BROWSER_HEADLESS`
    - `TANDEM_BROWSER_ALLOW_NO_SANDBOX`
    - `TANDEM_BROWSER_USER_DATA_ROOT`
    - `TANDEM_BROWSER_ALLOWED_HOSTS`
  - added typed engine browser tools:
    - `browser_status`
    - `browser_open`
    - `browser_navigate`
    - `browser_snapshot`
    - `browser_click`
    - `browser_type`
    - `browser_press`
    - `browser_wait`
    - `browser_extract`
    - `browser_screenshot`
    - `browser_close`
  - browser tools now register in both server mode and one-shot runtime mode; `browser_status` remains available even when browser execution is not runnable
  - added browser status surfaces across engine and clients:
    - `GET /browser/status`
    - browser summary block on `GET /global/health`
    - `tandem-engine browser status`
    - `tandem-engine browser doctor`
    - `POST /browser/smoke-test`
    - `tandem-browser doctor --json`
    - `tandem-browser serve --stdio`
    - TUI `/browser status` and `/browser doctor`
    - control-panel Browser Diagnostics settings card with install + smoke-test actions
  - browser artifacts now persist screenshots and large extracts as files/artifacts instead of returning large base64 payloads to the model
  - browser navigation/action flows now reuse host-allowlist policy and external-integration gating, and explicit session cancel/dispose paths now clean up tracked browser sessions
  - browser smoke testing now supports an engine-owned `example.com` validation flow that opens, snapshots, extracts visible text, and closes through the live sidecar path
  - browser sidecar transport failures now preserve stderr context and retry once after a dropped stdio connection before surfacing an error to clients

- **Orchestrator multi-run event fan-in and run registry (engine + control panel)**:
  - tandem-engine now exposes multiplex context-run SSE fan-in via `GET /context/runs/events/stream`, supporting one stream for many run IDs with cursor resume support.
  - context-run stream payloads now include a normalized envelope (run-scoped event metadata + cursor state) for deterministic client reconciliation.
  - control panel adds `/api/orchestrator/events` routing that prefers engine multiplex SSE and falls back to legacy per-run bridging when needed.
  - control panel adds `/api/orchestrator/events/health` for live stream health checks (mode, run count, cursor, and fallback status).
  - orchestration UI now uses a run registry store so multiple active/completed runs can coexist in one workspace instead of single-run overwrite behavior.
  - new dedicated Orchestrator page flow starts in prompt/workspace mode, then supports run switching and resume across existing runs.
  - orchestration and chat session history affordances now use a shared history icon treatment for consistency.

- **Blackboard promoted as run coordination backbone (engine + control panel)**:
  - extended context blackboard state with first-class task rows (`tasks`) including workflow/lineage fields, lease metadata, retries, and optimistic `task_rev`
  - added task patch ops to append-only blackboard log:
    - `add_task`
    - `update_task_lease`
    - `update_task_state`
  - added simplified task coordination endpoints:
    - `POST /context/runs/{run_id}/tasks`
    - `POST /context/runs/{run_id}/tasks/claim`
    - `POST /context/runs/{run_id}/tasks/{task_id}/transition`
    - `GET /context/runs/{run_id}/blackboard/patches`
  - added Pack Builder -> blackboard bridge via optional `context_run_id` on:
    - `POST /pack-builder/preview`
    - `POST /pack-builder/apply`
    - `POST /pack-builder/cancel`
    - `GET /pack-builder/pending`
  - when `context_run_id` is provided, pack-builder lifecycle updates are materialized as blackboard task patches/events in that context run
  - added automation v2 DAG projection into blackboard tasks:
    - `POST /automations/v2/{id}/run_now`, `GET /automations/v2/{id}/runs`, and `GET /automations/v2/runs/{run_id}` now sync node status into context blackboard tasks
    - `GET /automations/v2/runs/{run_id}` now returns `contextRunID` for the derived context-run projection
  - added skill-router blackboard mapping via optional `context_run_id` on:
    - `POST /skills/router/match`
    - `POST /skills/compile`
  - skill-router routing/compile outcomes now materialize as blackboard tasks/events in the target context run
  - desktop convergence step:
    - Tauri `orchestrator_get_blackboard` now prefers engine `/context/runs/{run_id}/blackboard` and only falls back to local orchestrator store for legacy compatibility
    - Tauri `orchestrator_get_blackboard_patches` now prefers engine `/context/runs/{run_id}/blackboard/patches` with legacy local fallback during migration
    - legacy Tauri read commands (`orchestrator_get_events`, `orchestrator_list_runs`, `orchestrator_load_run`) now prefer engine context-run APIs first, with local store fallback only for legacy runs
  - control-panel swarm SSE parity:
    - `/api/swarm/events` now streams both context run events and incremental blackboard patch events (`kind: "blackboard_patch"`) so UI refresh remains live even when only blackboard state changes
  - control-panel swarm task board overflow hardening:
    - long task/prompt titles are now clamped with explicit `More/Less` expansion and forced wrapping
    - prevents single oversized task prompts from stretching/swallowing the board layout
  - task lifecycle now emits run events (`context.task.created`, `context.task.claimed`, `context.task.started`, `context.task.completed`, `context.task.failed`, etc.) with `patch_seq` and `task_rev` for UI projections
  - validated backward compatibility for legacy persisted blackboards (payloads without `tasks` now deserialize with safe defaults)
  - replay/drift responses now include blackboard task parity checks (revision/count/status) and replay-vs-persisted blackboard payloads for debugging
  - control panel swarm route now forwards blackboard patch streams (`blackboardPatches`) and blackboard-aware task state
  - control panel `SwarmPage` now ships blackboard panel modes:
    - docked view
    - expanded view
    - fullscreen debug view
  - new control panel blackboard views include run status/current step/why-next-step context, decision lineage, agent lanes, workflow progress, artifact lineage, drift alerts, and patch feed
  - added regression tests for:
    - single-winner concurrent claims
    - `command_id` idempotency
    - optimistic `task_rev` mismatch handling
    - monotonic patch sequence and dedupe behavior
    - replay compatibility with task blackboard state

- **Run Engine commit boundary for context-run mutations**:
  - added `ContextRunEngine` as the single mutation path for context runs, with per-run locking and deterministic commit ordering for event append, snapshot persistence, and compatibility blackboard projection writes
  - task mutation endpoints now commit through the run engine instead of performing split writes directly:
    - `POST /context/runs/{run_id}/tasks`
    - `POST /context/runs/{run_id}/tasks/claim`
    - `POST /context/runs/{run_id}/tasks/{task_id}/transition`
    - `POST /context/runs/{run_id}/events`
  - authoritative run events now carry stable mutation metadata including `event_seq`, `revision`, `task_id`, and `command_id`
  - `events.jsonl` is now treated as the authoritative ordered per-run mutation history, while `run_state.json`, `blackboard.json`, and `blackboard_patches.jsonl` remain derived compatibility outputs
  - read paths now repair stale snapshots from `events.jsonl` and stale blackboard projections from `blackboard_patches.jsonl`, reducing crash-consistency drift
  - `POST /context/runs/{run_id}/events` now rejects `context.task.*` writes so task authority cannot bypass the run engine
  - added regression coverage for task race single-winner claims, task event metadata, snapshot repair, blackboard repair, command idempotency, and concurrent multi-run isolation

- **Automation creation UX simplified (control panel)**:
  - replaced the fragmented `Agents`, `Packs`, and `Teams` pages with a unified `Automations` hub (`AutomationsPage.tsx`)
  - added a 4-step wizard: **What?** (plain-English goal) → **When?** (visual schedule presets + custom cron) → **How?** (execution mode selector) → **Review & Deploy**
  - execution mode selector offers **Single Agent**, **Agent Team** (recommended default), and **Swarm** — guides users toward orchestration instead of single-agent loops
  - **My Automations** tab consolidates installed packs, scheduled routines, and recent run history in one view with run-now actions
  - **Teams & Approvals** tab surfaces active agent team instances and pending spawn approvals
  - legacy routes (`#/agents`, `#/packs`, `#/teams`) auto-redirect to `/automations` for backward compatibility
  - primary sidebar navigation reduced from 12 to 7 items: **Dashboard, Chat, Automations, Swarm, Memory, Live Feed, Settings**
- **Pack Builder orchestration/swarm execution mode**:
  - `pack_builder` tool now accepts `execution_mode` (`"single"` | `"team"` | `"swarm"`) and `max_agents` (integer, 2–32) fields
  - execution mode and orchestration config are stored in the routine `args.orchestration` block so the runtime can dispatch to an agent team or parallel swarm instead of a single agent loop
  - default mode changed from `"standalone"` to `"team"` — orchestrated agent teams are now the default for new automations created via the chat/pack-builder flow
  - tool schema updated to document these fields with enum constraints

- **Pack Builder zip storage race condition fixed**:
  - generated pack zip files are now saved to a persistent state directory (`~/.tandem/data/pack_builder_zips/` or `TANDEM_STATE_DIR/pack_builder_zips/`) instead of `std::env::temp_dir()`
  - previously, OS-level temp-dir cleanup between `preview` and `apply` phases caused silent `preview_artifacts_missing` failures when an `apply` was submitted seconds or minutes later
  - `retain_recent_plans()` now performs best-effort cleanup of evicted plan staging directories to prevent accumulation of stale zip archives

- **Semantic tool retrieval for tool-schema context reduction**:
  - added embedding-backed tool retrieval in `ToolRegistry` (`retrieve(query, k)`) to avoid sending all tool schemas on every provider call
  - retrieval indexing now tracks runtime lifecycle events:
    - startup bulk indexing via `tools.index_all().await`
    - incremental indexing on `register_tool` (covers MCP tools connected after startup)
    - vector cleanup on `unregister_tool` and `unregister_by_prefix` (covers MCP disconnect/refresh paths)
  - engine loop now uses semantic retrieval candidates by default (`TANDEM_SEMANTIC_TOOL_RETRIEVAL=1`) with `TANDEM_SEMANTIC_TOOL_RETRIEVAL_K` defaulting to `24` (aligned to existing expanded tool cap)
  - explicit policy tools are unioned from the full tool list so request/agent/session allowlist matches are not dropped by top-K retrieval
  - runtime system prompt now includes a compact connected MCP integration catalog (server names only) gated by `TANDEM_MCP_CATALOG_IN_SYSTEM_PROMPT`
- **MCP-first Pack Builder workflow in engine**:
  - added built-in `pack_builder` tool with `preview`/`apply` phases for generating installable Tandem packs from plain-English goals
  - external capabilities now map to MCP catalog connectors by default, with explicit fallback warnings when no connector match is available
  - generated pack `missions/` and `agents/` now include explicit discovered MCP tool IDs instead of abstract connector instructions
  - preview output now includes candidate connector list (with docs/transport/auth metadata), selected MCP mapping, required secrets, and approval requirements
  - apply path now supports MCP registration/connect, tool sync, pack install, and paused-by-default routine registration (explicit enable approval required)
  - persisted generated pack presets under `presets/overrides/pack_presets/` including connector selections, registered servers, required credentials, and selected MCP tools
- **Pack Builder API-first parity workflow contract**:
  - added first-class endpoints:
    - `POST /pack-builder/preview`
    - `POST /pack-builder/apply`
    - `POST /pack-builder/cancel`
    - `GET /pack-builder/pending`
  - endpoints delegate to the same `pack_builder` implementation as `/tool/execute` to keep preview/apply/cancel semantics identical across invocation paths
  - added persisted workflow + prepared-plan stores (`pack_builder_workflows.json`, `pack_builder_plans.json`) for restart-safe pending state
  - added thread-scoped pending-plan resolution (`session_id + thread_key`) to prevent cross-thread confirmation/apply collisions
  - added new regression coverage for preview/pending/cancel endpoint roundtrip, thread-scoped apply behavior, and missing-secret apply blocking
- **Pack Builder activation and routing surfaces**:
  - added default `pack_builder` agent profile with MCP-first system prompt and restricted tool allowlist
  - added channel-dispatcher intent routing for “create/build automation pack” style requests to `pack_builder` with explicit tool allowlist
  - added API-first channel command mapping for pack-builder workflows:
    - preview is created via `/pack-builder/preview` (no provider round-trip)
    - `confirm`/`ok` -> `/pack-builder/apply` for pending plan on that channel thread
    - `cancel` -> `/pack-builder/cancel`
    - `use connectors: ...` -> `/pack-builder/apply` with connector override
- **Tauri Pack Builder parity bridge**:
  - added desktop command/sidecar wrappers for:
    - `pack_builder_preview`
    - `pack_builder_apply`
    - `pack_builder_cancel`
    - `pack_builder_pending`
  - added desktop chat inline Pack Builder cards with direct apply/cancel actions that call the same workflow endpoints (no assistant text parsing required)
  - added Tauri regression tests validating preview/apply/cancel/pending endpoint wiring
- **Pack Builder observability counters**:
  - emit `pack_builder.metric` events for:
    - `pack_builder.preview.count`
    - `pack_builder.apply.count`
    - `pack_builder.apply.success`
    - `pack_builder.apply.blocked_missing_secrets`
    - `pack_builder.apply.blocked_auth`
    - `pack_builder.apply.cancelled`
    - `pack_builder.apply.wrong_plan_prevented`
  - each metric event includes inferred `surface` tag (`web`, `tauri`, `telegram`, `discord`, `slack`, `unknown`) plus session/thread/plan context
- **Preset index compatibility expansion**:
  - extended server preset index contract with `pack_presets` collection
  - updated control panel packs view to consume `pack_presets` safely
  - updated TUI preset index response model and `/preset index` output to include `pack_presets`
- **Routine MCP tool-picker UX in control panel**:
  - routine builder now includes a connected-MCP tool browser with server filter + text search
  - users can add discovered MCP tools to routine allowlist with one click instead of manual comma-separated typing
  - picker is backed by runtime `mcp.listTools()` discovery and mirrors currently connected/enabled servers
- **Regression coverage for MCP connector usage**:
  - added HTTP test asserting external-goal pack previews produce MCP tool mappings and mission steps containing `action: mcp.*`
  - added apply-phase test asserting explicit approval gating
  - added preset-registry tests for indexing and override lifecycle of `pack_preset`
  - added channel router tests for pack-builder intent detection and default-route fallback

### Fixed

- **Swarm continue/resume execution no-op in control panel**:
  - fixed executor behavior so runs with an already `in_progress` step no longer exit early when `/context/runs/{run_id}/driver/next` returns no `selected_step_id`
  - executor now resumes the active `in_progress` step and drives it through `prompt_sync` instead of silently idling
  - `POST /api/swarm/continue` and `POST /api/swarm/resume` now return execution diagnostics (`started`, `requeued`, `selectedStepId`, `whyNextStep`) to make no-op conditions visible
  - surfaced swarm `lastError` inline in `SwarmPage` to expose provider/session failures immediately
  - execution session creation now falls back to configured swarm provider/model when older runs lack persisted `model_provider`/`model_id`
- **Swarm fail-closed execution + model resolution hardening**:
  - added deterministic execution-model resolution precedence for swarm steps: context-run model -> swarm state model -> engine default provider/model
  - swarm start now fails fast when no provider/model can be resolved, instead of creating no-model runs that silently dispatch no LLM calls
  - `prompt_sync` dispatch now fails closed in control-panel swarm executor when no assistant output is produced (empty/no-op response), with explicit `PROMPT_DISPATCH_EMPTY_RESPONSE` diagnostics
  - added loop guard that stops swarm execution when a step remains non-`done` after completion (`STEP_STATE_NOT_ADVANCING`) to avoid hidden infinite replays on stale step state
  - added engine-compat reconciliation path: when step completion events do not materialize `done` immediately, control panel now patches run step state to `done` via engine API and emits `step_completion_reconciled`
  - `/api/swarm/status` now surfaces resolved model source + executor state/reason for immediate diagnosis
- **Swarm run list cleanup controls**:
  - added user-scoped cleanup endpoints in control panel:
    - `POST /api/swarm/runs/hide`
    - `POST /api/swarm/runs/unhide`
    - `POST /api/swarm/runs/hide_completed`
  - hidden runs are persisted in `~/.tandem/control-panel/swarm-hidden-runs.json` and filtered from `/api/swarm/runs` by default
  - `SwarmPage` now supports per-run `Hide` and bulk `Hide Completed` actions for decluttering old test runs without destructive engine-side deletion
- **Swarm completion/output observability in control panel**:
  - fixed false error surfacing on successful completion: executor no longer writes `all steps are done` into `lastError` when a run legitimately completes
  - Swarm page now renders a dedicated `Run Output` panel with latest completed step, session ID, and assistant output preview
  - task cards now resolve `Open Session` from step completion events (`session_id`) so users can always inspect generated outputs
  - run/task status badges now use semantic colors (`completed/done` = success, `failed` = error, `in_progress` = warning)
- **Engine startup stability during pre-ready phase**:
  - background server tasks now wait for runtime readiness before accessing `AppState` runtime-backed fields
  - fixes startup panic `runtime accessed before startup completion` that could mark control-panel connectivity unhealthy on boot
- **Duplicate tool-call guard tuning for write/edit workflows**:
  - increased default duplicate-signature retry limit for `write`/`edit`/`multi_edit`/`apply_patch` tool calls from `3` to `200`
  - preserves strict guardrails for `pack_builder` (`1`) and shell tools (`2`)
  - keeps global override behavior via `TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT`
- **OpenAI-compatible MCP tool schema normalization**:
  - normalized nested MCP function schemas before provider dispatch so tuple-style `items` arrays and object nodes without `properties` are rewritten into OpenAI-valid function parameter schemas
  - fixes `TOOL_SCHEMA_INVALID` 400 failures such as `mcp_airtable_list_records_for_table` when using models like `openai/gpt-5.3-codex` through OpenRouter
- **Semantic retrieval reliability for action-heavy prompts**:
  - engine now falls back from semantic top-K tool candidates to the full tool list when prompts indicate web research or email delivery and required tool families are missing from retrieval output
  - provider call routing telemetry now emits retrieval fallback fields (`retrievalEnabled`, `retrievalK`, `fallbackToFullTools`, `fallbackReason`) to aid diagnosis
  - tool calls not offered in the current turn are now rejected deterministically with explicit available-tool hints instead of attempting execution against unavailable/guessed tool names
  - final response guard now blocks false “email sent” claims unless a successful email-like tool action was actually executed in the run
- **Pack Builder autonomous routine defaults**:
  - pack-builder apply now defaults to autonomous routine registration for unattended automation:
    - generated routines default to `status: active`
    - generated routines default to `requires_approval: false`
    - API apply route approval flags now default to `true` for connector registration, pack install, and routine enablement
  - pack-builder generated routine YAML now emits `requires_approval: false` and apply output reflects enabled-by-default routine behavior
- **Provider stream connect timeout resilience**:
  - default provider stream connect timeout increased from `30_000 ms` to `90_000 ms` to reduce false startup/connect failures for scheduled routines and slower providers
  - updated default env examples and quickstart setup scripts to `TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS=90000`
- **Pack Builder permission friction across chat/channels**:
  - `pack_builder` tool is now allowed by default in baseline engine permission rules to prevent pack-creation requests timing out on first-use approval prompts
  - internal `pack_builder` apply-phase approvals remain required for connector registration, pack install, and routine enablement
- **Pack Builder retry-cost guardrail**:
  - duplicate-signature loop limit for `pack_builder` reduced to `1` to fail fast on repeated identical calls
  - added same-run duplicate-call guard for `pack_builder` to skip repeated execution attempts within one run cycle
- **Pack Builder UX + apply flow reliability**:
  - preview responses now return a concise human-readable summary (metadata remains structured JSON) instead of dumping raw/truncated JSON into chat output
  - fixed `connector_selection_required` to only trigger for unresolved external capabilities (built-in-satisfied external needs no longer force connector selection)
  - improved external email-action inference by detecting email-address targets in goals (even when the word “email” is absent)
  - apply flow now defaults connector registration to the plan-selected connectors (instead of an arbitrary single candidate)
  - safe previews now auto-apply by default (install pack + register routine paused) when no connector choice/secrets/manual setup are required
  - chat confirmation bridge: when a user replies with confirmation text after a Pack Builder preview, engine normalizes the next `pack_builder` call to `mode=apply` using the preview `plan_id` recovered from conversation context (prevents accidental new “confirm” packs across control panel, desktop, and channel threads)
  - pack-builder tool now also keeps last preview `plan_id` per session and interprets short confirmation goals (`ok`, `confirm`, `apply`, etc.) as apply of that session’s pending plan, so accidental `pack-builder-ok` installs are prevented even when model-side tool args are imperfect
  - apply output now renders explicit blocked summaries for:
    - `apply_blocked_missing_secrets`
    - `apply_blocked_auth`
  - cancelled plans are now terminal for later apply attempts (`plan_cancelled`)
  - control-panel chat now uses API-first pack-builder flow (preview/apply/cancel) for pack-intent and confirmation messages, reducing provider token burn and removing reliance on assistant paraphrase for state transitions
  - restored LLM-led initial pack creation flow (removed forced terminal short-circuit after `pack_builder` tool completion) so the assistant can continue in-chat guidance/questions when needed
  - control-panel chat now renders `pack_builder` state inline in the message thread (preview/apply cards with deterministic actions) instead of relying on side-rail-only visibility
  - channel dispatcher no longer short-circuits initial pack-intent messages into immediate preview text; deterministic `confirm`/`cancel` command mapping remains in place

## [0.4.0] - 2026-03-03

### Added

- **Engine-embedded MCP remote pack catalog**:
  - added generated MCP catalog assets under `crates/tandem-server/resources/mcp-catalog/` (index + per-server TOML manifests)
  - added MCP catalog module integration in server runtime (`mcp_catalog`)
  - added MCP catalog HTTP routes:
    - `GET /mcp/catalog`
    - `GET /mcp/catalog/{slug}/toml`
  - added generator script `scripts/generate-mcp-catalog.mjs` and control-panel convenience script `npm run mcp:catalog:refresh`
  - added curated official remote overrides for:
    - `github` (`https://api.githubcopilot.com/mcp/`)
    - `jira` (`https://mcp.atlassian.com/v1/mcp`)
    - `notion` (`https://mcp.notion.com/mcp`)
  - removed Docker-first default for GitHub MCP in curated catalog flows; default now uses official remote endpoint
- **Capability readiness preflight APIs (engine + SDK/UI)**:
  - added readiness request/response contracts in capability resolver (`CapabilityReadinessInput`, `CapabilityReadinessOutput`, blocking issues)
  - added readiness route:
    - `POST /capabilities/readiness`
  - readiness now classifies fail-closed blockers including:
    - missing required capability bindings
    - unbound required capabilities
    - missing/disconnected required MCP servers
    - auth-pending MCP tools
  - added TypeScript SDK support:
    - `client.capabilities.readiness(...)`
    - exported readiness input/output types in public package surface
  - added desktop-side command/wrapper support:
    - Tauri command `capability_readiness`
    - frontend wrapper `capabilityReadiness(...)`
- **Marketplace pack specification set** under `specs/packs/`:
  - `MARKETPLACE_PACK_REQUIREMENTS.md`
  - `PUBLISHING_AND_TRUST.md`
  - `STORE_LISTING_SCHEMA.md`
  - `DIFF_V1_TO_MARKETPLACE.md`
- **Pack identity hardening** in specs:
  - top-level immutable `pack_id`
  - top-level `manifest_schema_version`
  - explicit `contents` completeness validation for install-time checks
- **Pack trust/signing hooks** in specs:
  - root `tandempack.sig` signing contract
  - publisher verification tiers (`unverified`, `verified`, `official`)
  - client trust UX requirements and marketplace reject reason taxonomy
- **Marketplace-ready pack templates** under `examples/packs/`:
  - `skill_minimal_marketplace/`
  - `workflow_minimal_marketplace/`
  - each includes `tandempack.yaml`, README, content files, and marketplace assets
- **Modular preset specification set** under `specs/presets/`:
  - `PRESET_CONCEPTS.md`
  - `PRESET_STORAGE_AND_OVERRIDES.md`
  - `PROMPT_COMPOSITION.md`
  - `UI_REQUIREMENTS.md`
  - `API_CONTRACT.md`
  - `IMPLEMENTATION_PLAN.md`
- **Cross-UI preset and pack management contract**:
  - first-class PackManager API surface
  - shared PresetRegistry API surface for Desktop + Control Panel
  - attachment-driven pack detection/install UX contract for chat surfaces
- **Preset registry runtime/API foundation (server)**:
  - added layered preset indexer over built-ins, installed packs, and project overrides
  - added `GET /presets/index` route for shared backend preset discovery
  - added index contract with `skill_modules`, `agent_presets`, `automation_presets`, and source-layer metadata
  - added deterministic prompt composition engine and `POST /presets/compose/preview` route (`core -> domain -> style -> safety` ordering + stable composition hash)
  - added project override lifecycle endpoints for immutable-pack editing flow:
    - `POST /presets/fork`
    - `PUT /presets/overrides/{kind}/{id}`
    - `DELETE /presets/overrides/{kind}/{id}`
  - added capability summary route for composed presets/automations:
    - `POST /presets/capability_summary` (required dominates optional across agent + task scopes)
  - added export route for composed project overrides:
    - `POST /presets/export_overrides` (generates portable tandempack zip with marker manifest)
  - preset index metadata now includes parsed `publisher` and `required_capabilities` for library filtering
- **Initial PackManager runtime/API implementation (server)**:
  - added pack routes:
    - `GET /packs`
    - `GET /packs/{selector}`
    - `POST /packs/install`
    - `POST /packs/install_from_attachment`
    - `POST /packs/uninstall`
    - `POST /packs/export`
    - `POST /packs/detect`
    - `GET /packs/{selector}/updates` (stub)
    - `POST /packs/{selector}/update` (stub)
  - root-marker detection enforced via zip central directory entry `tandempack.yaml`
  - safe install extraction checks added (path traversal, size/count/depth limits)
  - zip-bomb hardening added via entry/archive compressed-to-uncompressed ratio checks
  - deterministic install/index paths under `TANDEM_HOME/packs` with atomic index writes
  - per-pack install/uninstall locking added so concurrent operations serialize by pack name while keeping index writes atomic
  - pack lifecycle events emitted (`pack.detected`, `pack.install.started|succeeded|failed`, `registry.updated`)
  - expanded HTTP regression tests for pack flows: marker-required detection, marker-negative zip behavior, deterministic install path/current pointer, and lifecycle events
  - pack inspect trust/risk summary now derives from installed content (`tandempack.sig` presence, publisher verification fields, capability/routine risk counts)
  - pack inspect now emits normalized verification badge tiers (`unverified`, `verified`, `official`) in API trust payload
  - pack inspect now includes a structured `permission_sheet` payload (required/optional capabilities, provider-specific dependencies, routine declarations, and risk level) for install UX
  - optional local secret scanning hook added during install (`TANDEM_PACK_SECRET_SCAN_STRICT`) with explicit `embedded_secret_detected` rejection on matches
  - pack update check/apply stubs now return structured `permissions_diff` plus `reapproval_required` flags for upgrade-approval UX wiring
- **Initial capability resolver runtime/API implementation (server)**:
  - added capability routes:
    - `GET /capabilities/bindings`
    - `PUT /capabilities/bindings`
    - `GET /capabilities/discovery`
    - `POST /capabilities/resolve`
  - data-driven bindings store under `TANDEM_HOME/packs/bindings/capability_bindings.json`
  - provider discovery from MCP tool catalog (`list_tools`) + local tool registry schemas
  - preference-aware resolver selection with default order (`composio` > `arcade` > `mcp` > `custom`)
  - alias-aware tool-name matching added (normalizes separator variants and supports per-binding `tool_name_aliases`)
  - expanded curated spine bindings for GitHub + Slack (data-driven defaults; no full-catalog mapping)
  - structured `missing_capability` conflict payload for unresolved required capabilities
  - added resolver regression coverage for explicit provider preference override when multiple providers satisfy the same capability
- **TypeScript SDK parity for packs + capabilities**:
  - added `client.packs` namespace methods for list/inspect/install/uninstall/export/detect/updates/update
  - added `client.capabilities` namespace methods for bindings get/set, discovery, and resolve
  - added public TypeScript types for pack/capability request/response shapes
- **Python SDK parity for packs + capabilities**:
  - added `client.packs` namespace methods for list/inspect/install/install_from_attachment/uninstall/export/detect/updates/update
  - added `client.capabilities` namespace methods for bindings get/set, discovery, and resolve
  - refreshed Python SDK README usage examples for pack/capability flows
- **Channel attachment pack ingestion flow (initial)**:
  - channel dispatcher now checks `.zip` attachments for tandem-pack marker via `/packs/detect`
  - trusted-source auto-install policy added via `TANDEM_PACK_AUTO_INSTALL_TRUSTED_SOURCES`
  - trusted zip uploads can auto-install through `/packs/install_from_attachment`
  - untrusted detections now return explicit install guidance to the user (no auto-install)
- **Rust UI network client parity (`tandem-tui`):**
  - added `EngineClient` methods for pack lifecycle endpoints (`packs_list/get/install/uninstall/export/detect/updates/update`)
  - added `EngineClient` methods for capability resolver endpoints (`capabilities_bindings_get/put`, `capabilities_discovery`, `capabilities_resolve`)
  - added `EngineClient` methods for preset flows (`presets_index`, `presets_compose_preview`, `presets_capability_summary`, `presets_fork`, `presets_override_put`)
  - added TUI `/preset` command family for desktop-native preset builder flows:
    - `/preset index`
    - `/preset agent compose ...`, `/preset agent summary ...`, `/preset agent fork ...`
    - `/preset automation summary ...`, `/preset automation save ...`
- **Control Panel Pack Library surface (`packages/tandem-control-panel`)**:
  - added dedicated `Packs` route/view for install/inspect/export/uninstall/update-stub actions
  - added control-panel UI hooks for capability discovery (`client.capabilities.discovery()`)
  - added pack install flows for URL/server-path sources from within control panel
  - added inspect-time trust/risk summary card in Pack Library (verification badge, signature state, capability/routine summary, provider-specific dependency count)
  - added update warning UX that surfaces `reapproval_required` on update checks/apply calls
  - added Skill Module Library section powered by `/presets/index` with text/publisher/required-capability filters
  - added Agent Preset Builder section with:
    - source preset selection
    - deterministic compose preview via `/presets/compose/preview`
  - replaced browser `window.confirm` destructive prompts with themed in-app confirmation modals for:
    - chat session deletion
    - pack uninstall
    - file deletion
  - moved toast notifications to top-center placement for consistent global visibility
  - regrouped Automations tabs/content under a single tab shell panel for cleaner section containment
    - capability summary via `/presets/capability_summary`
    - fork/save override flows via `/presets/fork` and `/presets/overrides/agent_preset/{id}`
  - added Automation Preset Builder section with:
    - task-agent binding rows (step-level agent swaps)
    - automation-level merged capability summary via `/presets/capability_summary`
    - override save flow via `/presets/overrides/automation_preset/{id}`
  - moved `Packs`, `Channels`, `MCP`, and `Files` out of primary sidebar navigation into a Settings-centric information architecture
  - added `Settings` tabbed sections (`General`, `Packs`, `Channels`, `MCP`, `Files`) so integration/asset management stays on the settings surface
  - added in-view migration prompts from Automations and legacy surfaces to direct users into Settings
- **Control panel MCP catalog and readiness surfaces (`packages/tandem-control-panel`)**:
  - MCP view now fetches and renders embedded catalog entries from `/api/engine/mcp/catalog`
  - added searchable “Remote MCP Packs” UI with pack apply actions and TOML open links
  - added readiness-check UI in MCP settings that calls `/capabilities/readiness`
  - Pack builders now enforce readiness preflight before saving agent/automation overrides
- **Desktop (Tauri) MCP catalog discoverability**:
  - added Tauri bridge command + wrapper for engine catalog retrieval (`mcp_catalog`)
  - Extensions/Integrations tab now includes searchable “Remote MCP catalog” view
  - catalog entries support one-click Apply to prefill remote name/URL and direct Docs open actions
- **Control Panel pack-event action surfaces (`packages/tandem-control-panel`)**:
  - added pack-specific event cards in `Live Feed` for `pack.*` events
  - added one-click actions from feed cards: open pack library, install from path, install from attachment
  - added `Chat` right-rail `Pack Events` stream with the same install/open actions
- **Pack implementation Kanban tracking**:
  - added `docs/internal/PACKS_PRESETS_IMPLEMENTATION_KANBAN.md` for phased execution tracking
- **Deterministic composition and governance rules** in specs:
  - stable prompt assembly ordering
  - capability/policy merge semantics
  - immutable installed pack sources with project-local fork/override editing model
  - explicit routine safety default (`disabled` on install)
- **Durable provider auth persistence (engine-wide)**:
  - added provider-key persistence in `tandem-core` with keychain-first storage and secure file fallback
  - engine startup now restores persisted provider keys into runtime provider config
  - `GET /provider/auth` now returns real per-provider auth status (`has_key`, `configured`, `source`)
  - `PUT /auth/{provider}` and `DELETE /auth/{provider}` now persist/remove provider keys for cross-restart behavior
- **Control panel provider wizard hardening**:
  - provider readiness now requires a stored key for key-based providers (non-`ollama`/`local`)
  - provider model tests now pin explicit provider/model and preflight for missing-key conditions
  - custom provider IDs are normalized consistently when saving/testing keys

### Fixed

- **Control panel packs runtime compatibility**: `Packs` view now falls back to direct engine endpoints when `state.client.packs`/`state.client.capabilities` namespaces are unavailable, preventing `Cannot read properties of undefined (reading 'list')` failures.
- **Settings tab UX**: Replaced generic button-like Settings section controls with dedicated tab styles (tab rail + active state) for clearer information hierarchy and better visual quality.
- **Settings container + icon hydration**: Settings tab content now renders inside the same Settings card container, and missing icon registrations (`package`, `sliders-horizontal`) were added so tab icons remain visible across section switches.
- **Control panel visual polish + theme parity**: Added shared theme-token application (`tandem.themeId`) and an Appearance selector in Settings so control panel visuals align with desktop themes, while upgrading shell controls/cards/nav/tabs to token-driven styling with smoother first-paint consistency.
- **Control panel first-load polish**: Replaced bare “Loading...” placeholders with themed skeleton surfaces and tokenized sidebar/brand chrome to reduce perceived jank during route/view switches.
- **Control panel icon coverage**: Expanded Lucide icon registration for packs/settings builders (`archive`, `copy-plus`, `sparkles`, `shield-check`, `arrow-up-circle`, `badge-check`, `binary`, `list`, `pencil`) to prevent missing icons after view/tab rerenders.
- **Settings tab icon persistence**: Fixed Settings subview icon hydration scope so top-level Settings tab icons no longer disappear when switching between `Packs/Channels/MCP/Files`.
- **Chat theme parity**: Refactored chat surface styles (sessions rail, message panes, composer, pills/chips, tool/approval/pack rails, upload progress, message pre blocks) to use shared theme tokens instead of hardcoded `zinc/slate` palettes so theme selection updates chat consistently.
- **Porcelain readability hardening**: Increased Porcelain text and border contrast and switched markdown rendering styles to token-based colors with light-theme overrides to prevent washed-out chat/content text.
- **Automations theme consistency**: Added `agents-theme` scoped token overrides for remaining `slate/zinc` utility islands in Automations/Wizard surfaces and converted agents tab/step chips to token-driven colors so Automations now follows theme selection.
- **Starter pack templates + personal tutorial**: Added three concrete importable starter templates under `examples/packs/` (`daily_github_pr_reviewer`, `slack_release_notes_writer`, `customer_support_drafter`) and a step-by-step personal walkthrough at `specs/packs/PERSONAL_TUTORIAL_FIRST_PACK.md`.
- **Settings icon hydration stability**: Added mutation-observer-backed icon rehydration in Settings so icons remain visible when switching tabs with async subview updates.
- **Automations tab style parity**: Updated `#/agents` section tabs to reuse the same underline tab styling and accessibility roles used in Settings.
- **Multi-theme selection restored**: Re-enabled full control-panel theme catalog (Web Control, Electric Blue, Emerald Night, Hello Bunny, Porcelain, Neon Riot) and restored Settings Appearance selector with quick theme swatches.
- **Provider test reliability**: Simplified Settings `Test Model Run` flow to a single async run path (removed sync+async double-run behavior), increased wait timeout, and clarified success messaging to prevent false “waiting” stalls with OpenRouter/default providers.
- **Provider test session isolation**: Provider test now runs in an internal non-workspace session (`/tmp/tandem-provider-test`) with internal title prefix, and chat session lists filter these internal sessions so users do not see provider-test artifacts.
- **Provider-key visibility mismatch in web control panel**: fixed false “No stored key detected” status caused by a stubbed auth-status route and non-durable auth writes.
- **Provider test ambiguity**: fixed settings “Test Model Run” paths that could execute with implicit defaults instead of the selected provider/model.

## [0.3.28] - 2026-03-01

### Added

- **Control panel MCP auth-mode UX**: Added MCP connection auth modes in web control panel (`auto`, `x-api-key`, `bearer`, `custom`, `none`) with Composio-aware auto-header behavior and inline auth-preview guidance.
- **Dashboard activity visuals**: Added control panel dashboard charts/summary cards for recent runs, status distribution, and automation/schedule activity to improve operator visibility.
- **Automations + Cost dashboard section**: Added a first-class dashboard block with token and estimated USD cost KPIs (`24h`/`7d`) plus top automation/routine cost breakdown rows.
- **Automations workspace IA refresh**: Refactored `#/agents` into tabbed `Automations` UX (`Overview`, `Routines`, `Automations`, `Templates`, `Runs & Approvals`) to reduce operator overload and improve task focus.
- **Walkthrough wizard for automation setup**: Added first-run + on-demand guided walkthrough for routine and advanced automation setup, with URL-deep-linkable tab/step state.
- **Persistent Automations V2 backend foundation**: Added additive `automations/v2` API surface with new persisted state files (`automations_v2.json`, `automation_v2_runs.json`), DAG run checkpoints, run-level controls, and SSE stream endpoint.
- **Per-agent model policy in V2 runs**: Added per-agent `model_policy` and runtime node-level model resolution so each agent can run different model tiers for cost/perf tuning.
- **Run-level token/cost accounting for automations**: Added token usage and estimated cost fields on routine/automation run records with provider-usage event aggregation and configurable rate via `TANDEM_TOKEN_COST_PER_1K_USD`.
- **Agent template write APIs**: Added `POST/PATCH/DELETE /agent-team/templates` so template selection and editing can be managed via API (not file-only).
- **SDK surface for V2 + template management**: TypeScript client now exposes `automationsV2` namespace and agent-team template create/update/delete methods.
- **Python SDK parity for V2 + template management**: Added `client.automations_v2` namespace and agent-team template create/update/delete methods in `tandem-client-py`.
- **Control panel Automation Builder V2 (first iteration)**: Added a new V2 builder in `agents` view with schedule setup, configurable agent count, per-agent model/skill/MCP/tool-policy inputs, DAG node editor, and create flow.
- **Automation preset packs (engineering + business)**: Added one-click V2 presets for `GitHub bug hunter`, `Code generation pipeline`, `Release notes + changelog`, plus marketing/productivity packs (`Marketing content engine`, `Sales lead outreach`, `Productivity: inbox to tasks`).
- **Guide docs V2 refresh (`guide/src`)**: Updated SDK + MCP automation docs to include `automations/v2`, per-agent model policy examples, and agent template CRUD examples for TypeScript/Python.

### Changed

- **Control panel login hero animation**: Replaced organic login animation with a uniform silicon-chip/data-flow motif for a cleaner and more consistent first-run visual style.
- **Chat session UX behavior**: “New chat” now auto-collapses the chat history sidebar to prioritize message composition space.
- **MCP error visibility in control panel**: MCP add/connect now reloads server state after failures and surfaces server `last_error` details directly in UI feedback.
- **Scheduler cron behavior**: Routine/automation scheduling now evaluates true cron expressions with timezone-aware next-fire computation and misfire planning.
- **Tool policy matching model**: Runtime allow/deny checks now support exact, wildcard, and prefix policies (`*`, `mcp.github.*`, `mcp.composio.*`) across session and capability gates.
- **Control panel V2 operations**: Added Automations V2 list/controls in `agents` view, including run-now, automation pause/resume, per-automation runs inspection, and run pause/resume/cancel actions.
- **Automation UX naming simplification**: Control panel copy now presents advanced automation flow tooling without exposing internal V2 labels to operators.
- **Control panel animation runtime**: Added lightweight `motion` animation library and panel transition animations for tabs/wizard interactions.
- **Control panel route transitions animation pass**: Extended motion-based transitions across routed views (`tcp-card`/list/nav-active) with reduced-motion awareness.

### Fixed

- **User-message render timing in chat**: Fixed delayed user message visibility; user messages now render optimistically at send-time instead of waiting for model response completion.
- **Tool activity rail population**: Fixed missing tool activity updates in control panel chat by broadening event parsing to include `session.tool_call`, `session.tool_result`, and message part tool event variants.
- **Approval queue stale-state cleanup**: Fixed stale/previous permission requests persisting in chat approvals rail; pending request filtering, one-time approval semantics (`once`), and refresh-on-session-change behavior now clear resolved approvals reliably.
- **Composio MCP stream response compatibility**: Fixed MCP runtime parsing for streamable/SSE JSON-RPC responses during remote discovery (`initialize` / `tools/list`), resolving `Invalid MCP JSON response: expected value at line 1 column 1` failures on Composio endpoints.
- **Routine hard-pause runtime semantics**: Pausing a `running` routine run now actively cancels tracked live session(s) and records cancelled session IDs in pause responses/events.
- **Swarm view route-stability fix**: Fixed Swarm page re-render race/leak where timer/SSE-triggered refreshes could leave Swarm content stuck after navigating to other views.
- **Control panel hash-query soft routing**: Fixed excessive full-shell re-renders on same-route hash/query changes (for example Automations tab/wizard clicks), reducing UI flash and stale render races.
- **Automation Builder model selection UX**: Per-agent provider/model fields now default from configured settings with provider/model dropdowns and explicit custom override inputs for clearer, lower-error setup.
- **Automation Builder custom model-provider fix**: Fixed agent-row controls so `Custom provider` and `Custom model` fields are editable/enabled when selected, and model options populate from selected/default provider.
- **Automation Builder policy field simplification**: Replaced ambiguous MCP/tool CSV inputs with connected-MCP selection and tool policy modes (`standard`, `read-only`, `custom`) while keeping advanced custom allow/deny override support.

## [0.3.27] - 2026-03-01

### Added

- **Identity configuration API + SDK parity**: Added `GET/PATCH /config/identity` with a typed personality preset catalog (`balanced`, `concise`, `friendly`, `mentor`, `critical`) and TypeScript/Python client support via `client.identity.get()/patch()`.
- **Dynamic tool router in engine loop**: Added intent-aware tool routing (`chitchat`, `knowledge`, `workspace_read`, `workspace_write`, `shell_exec`, `web_lookup`, `memory_ops`, `mcp_explicit`) with deterministic subset selection and MCP tool gating by default.
- **Per-request routing controls in prompt API**: Added `SendMessageRequest.tool_mode`, `tool_allowlist`, and `context_mode` (`auto|none|required` and `auto|compact|full`) for explicit runtime routing/context control from clients.
- **Tool routing/config telemetry events**: Added `tool.routing.decision` and `context.profile.selected` runtime events to expose selected-tool counts, routing pass/mode, and context compaction profile.

### Changed

- **Control panel channel configuration UX**: Channels view now loads persisted channel config, pre-fills existing values, and supports editing Discord `mention_only`/`guild_id` and Slack `channel_id` directly from the web control panel.
- **Channel diagnostics visibility in control panel**: Channel cards now surface backend `last_error` status so connector failures are visible without log tailing.
- **Discord usage guidance in control panel**: Added inline Discord guidance clarifying Tandem command style (`@bot /help`) and that Discord application slash commands are not registered by this integration.
- **Desktop Connections parity with web control panel**: Tauri Settings now adds Discord verification flow and richer setup diagnostics directly in desktop Connections UI.
- **Desktop runtime behavior guidance**: Added explicit Settings copy that channel listeners only run while the desktop app is open and the computer is awake, with an always-on deployment pointer.
- **Routine policy UX simplification (control panel)**: Added one-click `Allow everything` policy mode in routine create/edit that maps to unrestricted tools, external integrations enabled, and no approval gate.
- **Routine approvals operability in control panel**: Added Approve/Deny actions for pending routine/automation runs directly in routine cards, automations list, recent runs, and run inspector.
- **Telegram channel style customization**: Added configurable Telegram style profiles (`default`, `compact`, `friendly`, `ops`) with control panel support and server config exposure.
- **Config identity schema + soft legacy migration**: Engine config now supports canonical identity/personality structure under `identity.*` while accepting legacy `bot_name`/`persona` patch payloads and normalizing them into canonical fields.
- **Runtime assistant identity/personality injection**: Server prompt-context hook now injects bot name + personality preset/custom guidance into provider messages, with per-agent overrides applied to primary/subagent roles and hidden utility agents kept neutral.
- **Portal/control-panel identity naming**: VPS portal and control panel now fetch identity config and render bot/portal/control-panel labels from configured aliases instead of fixed hardcoded branding.
- **Protocol/header branding compatibility**: OpenRouter `X-Title` now supports `AGENT_PROTOCOL_TITLE` (canonical) with `TANDEM_PROTOCOL_TITLE` compatibility fallback, and auth now accepts both `x-agent-token` and `x-tandem-token`.
- **Guide docs identity/auth refresh**: Updated `guide/src` docs for identity/personality configuration and SDK usage, and switched curl examples to canonical `X-Agent-Token` while documenting `X-Tandem-Token` compatibility.
- **Identity settings UI across frontends**: Added bot-name/personality settings editors in Desktop (Tauri) Settings and Control Panel Settings, including personality preset selection and custom instructions save flow through `/config/identity`.
- **Onboarding wizard identity step (desktop)**: Added a first-run setup shortcut to the new Identity section so bot name/personality can be configured during initial setup.
- **Identity-aware chat labels**: Control Panel chat and Desktop chat message/header labels now use configured bot identity names instead of fixed “Assistant” labels.
- **Custom bot avatars across surfaces**: Added `identity.bot.avatar_url` support plus avatar upload controls in Desktop and Control Panel settings, with avatar rendering in Desktop chat and portal/control-panel shell/chat identity surfaces.
- **Server-side avatar normalization**: Identity avatar uploads are now normalized in server patch flow (data URL decode, resize to bounded dimensions, PNG re-encode), allowing larger client uploads without persisting oversized raw images.
- **Engine tool-loop guard tuning**: Engine loop now supports `TANDEM_MAX_TOOL_ITERATIONS` and `TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT` to reduce high-cost retry spirals on repeated tool signatures.
- **Default tool exposure behavior**: Engine `tool_mode=auto` now starts with a no-tools pass and escalates to a capped intent-matched subset only when needed, instead of publishing the full tool catalog on every call.
- **Context sizing behavior for trivial prompts**: Added compact context profile selection for short/simple prompts and server-side memory-injection skip heuristics for low-signal greetings/chitchat to reduce token overhead.
- **SDK prompt parity for routing controls**: TypeScript and Python session prompt clients now support passing routing options (`toolMode`/`toolAllowlist`/`contextMode`) to `prompt_async`.
- **Provider stream fail-safe behavior**: Engine provider streaming now enforces configurable connect/idle timeouts to fail stuck upstream calls deterministically and release active runs instead of hanging sessions.
- **Timeout defaults aligned across engine and installers**: Engine runtime defaults now use `TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS=5000`, `TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS=30000`, and `TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS=90000`; quickstart/VPS/control-panel installers now write these into default engine env files.
- **Shell tool timeout guardrail**: Added `bash` tool timeout support (`timeout_ms` arg and `TANDEM_BASH_TIMEOUT_MS`, default `30000`) so stalled shell commands cannot keep runs active indefinitely.
- **Control panel chat stream resiliency**: Chat stream watchdog now uses longer no-event/max-window thresholds and a run-settlement wait path before surfacing a stuck-run failure.

### Fixed

- **npm publish provenance metadata for control panel**: Added missing `repository` metadata in `packages/tandem-control-panel/package.json` so npm provenance validation accepts `@frumu/tandem-panel` CI publishes.
- **Discord allowlist identity matching**: Discord inbound authorization now supports user ID, username, global name, and mention-style identity entries (for example `@name`, `<@id>`, `<@!id>`) instead of ID-only matching.
- **Discord non-response misconfiguration path**: Fixed common “connected but not responding” states caused by hidden/default Discord settings by exposing mention-only and guild filters in the control panel.
- **Channel setup consistency across surfaces**: Added unified Discord verify backend (`POST /channels/{name}/verify`) and desktop integration so setup checks are consistent across web and Tauri flows.
- **Telegram MarkdownV2 heading readability**: Telegram formatter now renders markdown headings as styled heading text instead of leaking literal escaped `###` markers in bot replies.
- **Telegram MarkdownV2 chunk boundary robustness**: Message chunking now prefers safe markdown-entity boundaries to reduce formatting breakage and parse-mode failures when splitting long responses.
- **Runaway repeated tool-call retries**: Engine now guards duplicate non-read-only tool signatures (including repeated `bash` calls), emits deterministic loop-guard terminal messaging, and stops wasteful repeated provider/tool cycles.
- **Tool-context bloat on lightweight prompts**: Short/simple prompts no longer incur full tool list + memory context injection by default, reducing unnecessary token burn and repeated tool-call churn.
- **Startup-time runtime panic in prompt context hook**: Server prompt augmentation now fail-opens when runtime state is not yet installed, preventing `runtime accessed before startup completion` panics during startup races.
- **Cross-channel stuck-active runs**: Engine now times out stalled provider streams and returns terminal failure instead of leaving runs indefinitely active (which previously cascaded into session/run conflicts across web/Discord/Telegram).

## [0.3.26] - 2026-02-28

### Added

- **Engine storage file listing API**: Added `GET /global/storage/files` to enumerate files under the engine storage directory (`TANDEM_HOME`/`TANDEM_STATE_DIR`/resolved shared paths fallback), with `path` and `limit` query support.
- **Channel attachment local-ingest parity**: Discord and Slack adapters now download inbound attachments to local `channel_uploads` storage and populate attachment path metadata consistently with Telegram.
- **Provider multimodal message shape**: Added attachment-aware provider chat message model with image attachment support for OpenAI-compatible request payload construction.

### Changed

- **Channel -> engine prompt submission for attachments**: Channel dispatcher now sends uploaded attachments as explicit `file` message parts to `/session/{id}/prompt_async` instead of only text synthesis.
- **Attachment prompt guidance policy**: Channel synthesized prompts now direct models/tools to analyze supported attachment MIME types directly and report unsupported-format capability requirements explicitly.
- **OpenAI-compatible request building**: Provider stream path now emits multimodal `messages[].content` arrays when image attachments are present, while preserving text-only behavior when no attachments exist.
- **Engine runtime attachment routing**: Engine loop now maps inbound `MessagePartInput::File` image parts into provider attachments for first-iteration user context, including local-file-to-data-URL conversion with size caps.

### Fixed

- **“Image-capable model but image not analyzed” channel gap**: Attachments are now propagated end-to-end from channels through engine/provider dispatch instead of being reduced to metadata-only text markers.
- **Slack private attachment accessibility**: Slack file downloads now authenticate with bot bearer token for `url_private(_download)` resources before local persistence.
- **Directory traversal hardening on storage listing**: `/global/storage/files` rejects absolute and parent-directory path traversal segments in query path input.

## [0.3.25] - 2026-02-28

### Added

- **MCP auth challenge extraction hardening**: Runtime now prefers structured auth fields from MCP payloads (for example `structuredContent.message` / `structuredContent.authorization_url`) before generic text blobs, improving challenge fidelity across providers like Arcade.
- **Quickstart UI preference persistence**: Added persistent storage for chat `Auto-allow all` approval mode so the toggle survives portal reloads/restarts.
- **Guard-budget diagnostics tests**: Added engine/runtime test coverage for per-run guard-budget classification and MCP auth message normalization/priority parsing.
- **Global memory persistence substrate (SQLite + FTS5)**: Added persistent global memory record storage in `memory.sqlite` (`memory_records` + `memory_records_fts`) with dedup indexes, FTS-backed search, and promote/demote/delete/list APIs over durable records.
- **Always-on memory ingestion events and pipeline**: Added server memory ingestor subscriber that captures memory candidates from run/tool/decision/auth event streams and emits canonical write lifecycle events:
  - `memory.write.attempted`
  - `memory.write.succeeded`
  - `memory.write.skipped`
- **Run-time memory observability events**: Added structured retrieval/injection telemetry emitted during provider planning:
  - `memory.search.performed` (scores, sources, latency)
  - `memory.context.injected` (count, token-size estimate)
- **Memory demotion API surface**: Added `POST /memory/demote` to move memories back to private/demoted state without hard delete.
- **SDK memory API parity updates (TS + Python)**: Updated both client SDKs for new memory payload shapes (`content`/`source_type`/`run_id`, promote/put response variants) and added explicit memory demote client methods.
- **Protocol contract alignment for global memory**: Updated HTTP/event contracts and OpenAPI summaries to include `/memory/demote`, global-memory route semantics, and memory lifecycle/search/injection SSE events.
- **Guide and SDK documentation refresh for global memory**: Updated root README (EN/zh-CN), SDK READMEs, and guide pages (`sdk/*`, headless endpoints, tools reference, engine command examples) to reflect always-on global memory semantics and demote support.

### Changed

- **MCP auth messaging output quality**: Sanitized/truncated auth-required messages emitted from runtime so user-facing chat responses avoid large escaped JSON or provider-internal instruction blobs.
- **Quickstart run lifecycle handling**: Before sending a new prompt, quickstart now best-effort cancels an existing active run (`cancelRun` / `cancel`) to avoid stale-run carryover behavior in active sessions.
- **Provider/tool compatibility normalization**: Hardened OpenAI-compatible tool publishing path with function-name sanitization, alias round-tripping, and function-parameter schema normalization for stricter provider validators.
- **Memory API backend behavior**: Server `/memory/*` routes now use durable global-memory DB records instead of in-process map state, preserving data across process restarts.
- **Memory retrieval timing model**: Retrieval/injection now runs in the engine planning loop (via server-installed prompt-context hook), so each provider iteration can pull fresh relevant memory rather than run-start-only context.
- **Prompt context assembly for planning loops**: Memory context is appended as bounded `<memory_context>` system material during provider planning iterations, with per-iteration scoring thresholds and observability.
- **Secret/PII safety on memory write path**: Added stronger write-path scrubbing/blocking before persistence (pattern-based redaction + hard-block markers), and redaction status/count metadata tracking on persisted records.

### Fixed

- **Runaway MCP/tool-call churn in a single run**: Engine loop now short-circuits additional MCP tool execution in auth-pending conditions and applies stricter fail-fast behavior when guard budget is exceeded to prevent high-cost retry spirals.
- **Misleading session-scoped budget fallback wording**: Guard-budget terminal behavior is now deterministic and explicitly run-scoped, reducing model-generated “session limit” confusion.
- **Model routing in quickstart sends**: Quickstart run submission path now consistently sends explicit provider/model overrides via `prompt_async`, aligning selected UI model with actual provider traffic.
- **OpenAI-compatible strict schema/tool-name 400s**: Fixed invalid tool-name and function-parameter schema edge cases that caused upstream `invalid_request_error` failures during MCP-heavy runs.
- **MCP auth loop UX degradation**: Reduced repeated noisy auth output and improved event/message behavior for `mcp.auth.required` / `mcp.auth.pending` flows across engine and web chat.

## [0.3.24] - 2026-02-27

### Added

- **MCP auth diagnostics contract surfaced to clients**: Clarified and propagated runtime auth-required signaling via `mcp.auth.required`, including richer auth challenge metadata handling in engine/web chat paths.
- **MCP server auth/session state fields**: MCP server state now tracks auth/session continuity fields used by diagnostics and reconnect flows:
  - `last_auth_challenge`
  - `mcp_session_id`
- **Release workflow control for Python packages**: Added an independent PyPI publish toggle in registry publish workflow so Python package release can be enabled/disabled separately from npm publish.

### Changed

- **MCP reconnect/refresh lifecycle behavior**: MCP connect/refresh failure handling now clears stale cached tools/session state and returns deterministic reconnect outcomes for downstream clients.
- **MCP call argument handling (engine-wide)**: Runtime now normalizes MCP `tools/call` argument keys against tool schema to tolerate common alias/key-shape drift (for example camelCase/snake_case and `name -> task_title` style recovery for required fields).
- **Agent quickstart startup/runtime flow**: Quickstart provider/chat bootstrap behavior now enforces healthier startup gating and surfaces richer run/auth diagnostics for chat sessions.
- **Docs and README alignment**: Updated top-level/docs artifacts for current SDK/runtime behavior, including docs URL and README/README.zh-CN parity refresh.
- **Publish pipeline hardening for TS client**: Updated publish CI flow for `@frumu/tandem-client` to ensure required build/declaration prerequisites are present in publish jobs (`typescript`, DOM lib compatibility, `tsup` + `tsc` path).

### Fixed

- **MCP auth/retry robustness regressions**: Fixed MCP auth challenge handling and reconnect/refresh edge cases that could leave stale cache/tools or unclear auth state after failures.
- **Agent quickstart blank/non-response chat regressions**: Fixed chat flows where runs could appear blank/non-responsive; run failures now surface with clearer diagnostics in UI.
- **Agent quickstart provider setup gating/model handling**: Fixed startup/provider gating and model-selection handling to avoid broken entry states when config is incomplete.
- **Quickstart proxy auth/key routing**: Fixed portal auth handling scope for `/engine` proxy and key/env consistency behavior in quickstart deployment scripts/config.
- **TS client npm publish failures**: Resolved publish failures caused by provenance metadata mismatch and missing CI build/declaration dependencies.

## [0.3.23] - 2026-02-27

### Added

- **First official SDK release announcement (TS + Python)**: `0.3.23` is the first Tandem release that formally ships and announces both client SDK packages:
  - TypeScript: `@frumu/tandem-client`
  - Python: `tandem-client`
- **TypeScript SDK token lifecycle mutator**: Added `TandemClient.setToken(token)` in `@frumu/tandem-client` for explicit auth-token updates across future HTTP/SSE calls.

### Changed

- **Agent quickstart SDK migration**: Updated `examples/agent-quickstart` to current TS SDK namespaces and models (providers/channels/permissions/messages/tools/routines), replacing legacy API usage and aligning event/run-state handling.
- **Quickstart client auth wiring**: Refactored quickstart API/auth context to explicit token management helpers (`setClientToken`, `clearClientToken`, `verifyToken`) and removed direct internal-client mutation patterns.
- **Release bump parity (shell + PowerShell)**: Updated `scripts/bump-version.sh` and `scripts/bump-version.ps1` so version bumps include:
  - `packages/tandem-client-ts/package.json`
  - `packages/tandem-client-py/pyproject.toml`

### Fixed

- **Agent quickstart TypeScript build failures**: Resolved compile errors caused by SDK API-shape drift (including invalid imports, outdated method paths, strict type mismatches, and stale event/tool/run field assumptions).

## [0.3.22] - 2026-02-26

### Added

- **Engine-first context-driving runtime surfaces**: Expanded context-run APIs and client wiring for event replay, checkpoint access, deterministic next-step selection, and todo->step synchronization so Desktop/TUI consume the same engine-owned truth.
- **Desktop Blackboard panel system**: Added shared Blackboard panel module for Orchestrator + Command Center with docked/expanded/fullscreen modes, decision lineage views, drift details drawer, search/filter controls, and keyboard-first navigation.
- **Blackboard contract test suite**: Added dedicated `test:blackboard` coverage for projection/filtering, follow-mode state transitions, refresh policy, and drift drawer state behavior.

### Changed

- **Blackboard refresh behavior**: Switched to debounced event-driven blackboard refresh with sequence watermarking (`last_blackboard_refresh_seq`) and relevant-event family gating to reduce redundant fetch pressure during long runs.
- **Orchestrator/Command Center parity**: Unified both surfaces on shared blackboard UI/state/policy helpers to keep behavior consistent across run-control entrypoints.
- **Two-pass orchestrator planning**: Planning now performs an analysis pass before DAG generation to improve task quality and reduce low-context first plans.
- **Context-aware execution prompts**: Builder prompts now include continuation context from context-pack summaries so retries/resumes stay on-track instead of restarting from scratch.
- **Planner tool contract alignment**: Planner prompt tool inventory now matches actual runtime tools (`glob/read/write/edit/apply_patch/websearch/webfetch/webfetch_html/codesearch`) and explicitly plans web research when local source material is sparse.
- **Tool path normalization hardening**: File-path normalization now rejects synthetic placeholders like `files/directories` and `tool/policy`, recognizes document extensions (`.pdf/.docx/.pptx/.xlsx/.rtf`), and avoids deriving file paths from assistant narrative context.
- **Release metadata bump**: Updated app/runtime package versions to `0.3.22` for desktop/TUI release alignment.

### Fixed

- **Follow-mode predictability**: Follow now auto-focuses only on new `meta_next_step_selected` decisions and pauses on manual navigation, preventing jumpy recentering on unrelated event noise.
- **Drift/debug visibility**: Added actionable drift and checkpoint navigation affordances with copyable debug payloads for incident triage.
- **Task/session continuity on restart and retry**: Orchestrator now restores task session bindings from checkpoints and preserves failed-task session context by default during retry.
- **Planning/execution token accounting visibility**: Budget token usage now records prompt+response estimates for planner analysis/planner/builder/validator calls, preventing misleading near-zero token displays.
- **Blackboard event coverage for engine runs**: Blackboard projection and refresh logic now recognize orchestrator event families (for example `context_pack_built`, planning/task/run events), improving live context visibility in both Orchestrator and Command Center.
- **Write-required task fail-fast behavior**: Orchestrator now fails fast with explicit errors when builder recovery performs no tool calls or only read-only tool calls on tasks that require file modifications.
- **Read/write sandbox diagnostics and Windows path handling**: Tool path policy now accepts Windows verbatim paths (`\\?\...`) when in-workspace, performs lexical in-workspace checks for non-existent targets, and returns actionable denied-path diagnostics (workspace root/effective cwd/suggested path).
- **Read tool error transparency**: `read` now returns explicit failure reasons (`path_not_found`, `path_is_directory`, `read_text_failed`) instead of silent empty output, reducing retry loops and validator misclassification.
- **Blackboard task-trace visibility**: Blackboard projection/filtering/refresh now includes `task_trace` events (for example `FIRST_TOOL_CALL: glob`) and Orchestrator surfaces paused-run error context directly in the panel.
- **Validator evidence gating for file-output tasks**: Validation prompts now include concrete snippets from changed files and explicitly fail when completion is claimed without verifiable output content.
- **Malformed required-arg tool-call fast timeout**: Pending tool calls that likely omitted required args (for example repeated `read {}`) now time out quickly instead of hanging for long watchdog windows.
- **Provisional tool-start timeout guardrails**: Provisional/partial tool starts (for example `read` before finalized args arrive) no longer trigger the malformed-args fast-timeout path, reducing false `TOOL_TIMEOUT` failures during streamed tool-call assembly.
- **Sidecar `run_conflict` recovery**: Message send/start-run now honors `retryAfterMs` and retries conflict responses instead of surfacing immediate hard failures during active-run handoff.
- **Stale active-run conflict breaker**: Repeated `run_conflict` responses for the same active run now trigger active-run probing and targeted cancellation of stale runs, preventing long conflict retry loops and session thrash.
- **Provider stream failure classification**: Server now maps upstream provider stream/server failures to structured `PROVIDER_SERVER_ERROR` codes for clearer diagnostics and retry behavior.

## [0.3.21]

### Changed

- **Global storage path standardization**: Unified global Tandem state fallbacks across engine/runtime/server/channels/skills/core to OS-native app-data roots (`.../tandem`) with consistent `data`/security/config placement and reduced ad-hoc relative-path defaults.
- **Global storage override support**: Added `TANDEM_HOME` support to shared storage path resolution so operators can pin a canonical global Tandem root explicitly in CI/server environments.

### Fixed

- **Crates publish-order dependency gap**: Added `crates/tandem-agent-teams` to deterministic publish-order manifests/scripts before `crates/tandem-tools`, preventing crates.io publish failures from unresolved intra-release dependency lookup.

## [0.3.20] - 2026-02-25

### Changed

- **TUI agent fanout mode handoff**: `/agent fanout` now auto-switches `plan -> orchestrate` before teammate delegation to reduce plan-mode gating/approval churn during coordinated multi-agent runs.
- **TUI agent-team workflow integration**: Added coordinated fanout team bootstrapping (`TeamCreate` + delegated `task` routing), mailbox/member session binding, and teammate-target normalization (`A2`/`a2`/`agent-2`) to improve local team execution consistency.

### Fixed

- **TUI small-paste readability and composer rendering**: Small pastes (1-2 lines) now insert directly without `[Pasted ...]` tokens, CRLF is normalized, and composer height now expands correctly for explicit newlines (fixes overlapped/cropped second-line input rendering).
- **OpenAI-compatible stream parsing robustness (OpenRouter, tool flows)**: Provider streaming now accepts both `choices[].delta` and `choices[].message` payload shapes for text and tool-call fields, preventing empty assistant replies and missed tool execution when providers return non-delta message chunks.
- **Provider token-budget safety default**: OpenAI-compatible provider requests now set an explicit bounded `max_tokens` default (`2048`, overridable via `TANDEM_PROVIDER_MAX_TOKENS`) to prevent accidental large-budget requests (for example `65536`) that trigger 402 credit failures on simple prompts/tool invocations.

## [0.3.19]

### Changed

- **VPS Stress Lab parity upgrade**: Server-side stress scenarios (`remote`, `file`, `inline`) now execute true async run flows and wait for run completion, aligning Tandem latency measurement with end-to-end provider/tool execution instead of submission-only timing.
- **Cross-system benchmark comparison in portal**: Added OpenCode benchmark ingestion (`latest`, `history`, `by-date`, `health`) and in-UI Tandem vs OpenCode delta reporting for matched stress scenarios.
- **OpenCode benchmark runtime mode**: Added warm attached execution support (`opencode serve` + `run --attach`) to avoid cold-start CLI overhead during repeated benchmark runs.
- **Portal Caddy/API routing compatibility**: Standardized `/api/v1` compatibility behavior and health endpoint support for external benchmark-service integration.
- **TUI interaction model and keyboard UX refresh**: Updated request-center/question handling, mode/status highlighting, sessions UX hints, and command ergonomics (including agent fanout and session delete affordances) for clearer in-terminal workflows.
- **TUI dependency/runtime modernization**: Migrated terminal stack to `ratatui 0.30`/`crossterm 0.29`, replaced third-party throbber dependency with local spinner components, and aligned render paths for better cross-platform behavior.

### Fixed

- **Engine request observability coverage**: Added explicit timing/slow-request instrumentation for `session.command`, `session.get`, and `session.list` server routes to improve bottleneck diagnosis under load.
- **Stress chart rendering stability**: Fixed NaN polyline generation in Stress Lab line charts when metric series are all-zero or sparsely initialized.
- **Server-side model selection for stress runs**: Portal server stress runner now resolves and injects an explicit provider/model for prompt scenarios, preventing misleading providerless-like latency readings in LLM tests.
- **TUI plan/request deadlock and queue conflicts**: Fixed several plan-mode edge cases that could cause repeated `409 session has active run` loops after question/approval handoffs by routing follow-up prompts through safe queue semantics.
- **TUI question answer fidelity and visibility**: Fixed option selection/confirm behavior in compact request mode and added explicit confirmation output showing submitted question answers.
- **TUI paste reliability on Windows**: Added burst-aware paste handling with tokenized placeholders to prevent accidental line-by-line replay, unintended submits, and input corruption during large clipboard pastes.
- **TUI session/task persistence parity**: Restored task reconstruction when re-opening historical sessions by broadening tool-part parsing (`tool`, `tool_call`, `tool_use` forms), so plan sessions reopen with their task lists instead of plain chat-only views.

## [0.3.18]

### Fixed

- **Provider model override from env API-key bootstrap**: Setting `OPENROUTER_API_KEY` no longer forces `providers.openrouter.default_model` to `openai/gpt-4o-mini` in the env layer.
- **Model-selection persistence in VPS/web deployments**: Engine now preserves configured provider default model (for example `z-ai/glm-5`) unless an explicit model env var is set.
- **Config env-layer behavior clarity**: OpenAI-compatible env bootstrap now treats API key and model override separately; model override is applied only when explicitly provided via env.

## [0.3.17]

### Changed

- **Channel session bootstrap defaults**: Channel-created sessions now include a practical default permission set so long-running channel workflows do not silently stall waiting for hidden permission prompts.
- **Channel SSE attach strategy**: Channel run streaming now subscribes at the session level for better compatibility with engines that emit session-scoped events.
- **Portal run observability UX**: Web example dashboards now include richer watchdog/runtime traces and clearer status transitions around stream-ready, run-activity, and poll-finalized completion paths.

### Fixed

- **Run stream event parsing for channels**: Channel dispatcher now consumes `message.part.updated` text deltas and additional terminal run lifecycle variants, improving reply reliability in chat connectors.
- **Telegram diagnostics quality**: Telegram adapter now logs richer poll failure diagnostics (debug transport context + non-success HTTP status/body preview) for production debugging.
- **Permission/approval visibility in portal**: Added global pending-approval visibility and one-click approval action in the portal shell, plus clearer no-pending messaging.
- **Portal SSE reliability in examples**: Web examples now prefer session-level event streaming to avoid runs appearing as `connected/ready` with no live deltas.

## [0.3.16]

### Fixed

- **What's New release-note mismatch**: Desktop now fetches release notes for the installed app tag from GitHub at runtime instead of relying on a hardcoded local markdown import.
- **Safe fallback behavior**: If release-note fetch fails or a version body is unavailable, the What's New overlay shows no stale notes and links users to the latest GitHub release page.
- **Plan task execution integrity**: "Execute Pending Tasks" now uses a strict completion contract and validates task completion from actual `todowrite`-driven todo status updates, not assistant text claims.
- **Pending-task payload correctness**: Chat execution now receives pending-only tasks (instead of all todos), so execution prompts and sidebar counts stay consistent.

## [0.3.15]

### Added

- **Channel slash-command expansion**: Added new channel commands for run control and observability:
  - `/run`
  - `/cancel` (and `/abort` alias)
  - `/todos` (and `/todo` alias)
  - `/requests`
  - `/answer <question_id> <text>`
  - `/providers`
  - `/models [provider]`
- **Channel model switching**: Added `/model <model_id>` to update the default provider's active model from connected chat channels without requiring provider/token switching flows.

### Changed

- **Channel command docs**: Updated channel integration docs/help text to reflect expanded command coverage and model control from chat channels.
- **Provider settings model UX**: Anthropic/OpenAI settings now use text-input-first model selection with updated current model suggestions and clearer provider-specific placeholders.

### Fixed

- **Custom provider config sync (llama-swap/OpenAI-compatible)**: Desktop now writes enabled custom provider endpoint/model into engine config (`providers.custom`) and updates default-provider routing when custom is selected.
- **Custom provider registry support**: Engine provider registry now accepts custom/non-built-in provider IDs from config instead of falling back to `local` only.
- **Custom endpoint normalization hardening**: OpenAI-compatible base URL normalization now handles trailing `/v1`, repeated `/v1/v1`, and full-path inputs (for example `/v1/chat/completions`) to prevent malformed request URLs.
- **Transient provider reachability resilience**: Added short retry behavior for connection/timeout failures when calling OpenAI-compatible providers, reducing one-off local gateway startup hiccups.
- **Settings save feedback**: Saving Custom Provider in Settings now shows explicit success/error feedback instead of silently completing with no user confirmation.
- **Provider error diagnostics**: Provider call failures now include clearer endpoint + failure-category guidance (`connection error` / `timeout`) to speed up local gateway troubleshooting.

## [0.3.14]

### Fixed

- **Endless sidecar update prompt hotfix**: Desktop now avoids preferring stale AppData sidecar binaries when the bundled engine version is newer, preventing false "you have v0.3.0" update loops after upgrading.
- **Version label rendering**: Update UI normalizes engine version labels to avoid duplicated prefixes like `vv0.3.12`.

## [0.3.12]

### Fixed

- **MCP runtime compatibility hotfix**: Desktop now falls back to MCP server `tool_cache` via `GET /mcp` when legacy/mixed engine builds return `404` for `GET /mcp/tools`, preventing Extensions MCP tab load failures (`Failed to load MCP runtime`).
- **Registry publish pipeline ordering**: Fixed crates publish order and dependency coverage in CI scripts/workflow so tandem workspace crate dependencies publish in valid sequence.

## [0.3.11]

### Changed

- **Provider model selection now catalog-backed in Settings**: OpenAI, Anthropic, and OpenCode Zen settings now prefer live engine catalog model IDs when available instead of static-only model lists.

### Fixed

- **Custom provider routing and validation (#14)**: Fixed `custom` provider resolution in desktop Tauri routing so explicit custom provider/model selections dispatch correctly for chat and automation flows.
- **Custom provider selection persistence**: Saving/enabling a custom provider now updates `providers_config.selected_model` when a model is provided, preventing silent fallback to unrelated provider/model selections.
- **Release notes fallback in Settings**: Release notes now fall back to updater `latest.json` metadata when GitHub Releases API fetch fails.
- **Bundled sidecar version reporting**: Sidecar updater status now reports bundled-engine version from app package metadata, avoiding stale beta values from old downloaded sidecar records.

## [0.3.9]

### Added

- **Memory Consolidation**: Added opt-in LLM summarization of session memory using the cheapest available configured provider (prioritizing local/free options like Ollama, Groq, OpenRouter).
- **Channel Tool Policy**: Added explicit policy controls (`allow_all`, `deny_all`, `require_approval`) for tool execution in messaging channels, configurable via `config.json` or `TANDEM_CHANNEL_TOOL_POLICY` env var.
- **Richer Channel Session Metadata**: Upgraded channel session tracking to persist detailed `SessionRecord` metadata (created/last-seen timestamps, channel, sender) instead of bare IDs.
- **Headless Web Admin UI (embedded, single-file)**: Added an embedded `/admin` web interface served directly by `tandem-server` using a baked-in `admin.html` shell (no external assets/build pipeline at runtime).
- **Realtime Admin UX**: Added SSE-driven UI refresh behavior (with polling fallback) for channel/session/memory visibility in the headless admin surface.
- **Channel Admin API surface**: Added channel-management endpoints for headless control:
  - `GET /channels/status`
  - `PUT /channels/{name}`
  - `DELETE /channels/{name}`
  - `POST /admin/reload-config`
- **Memory Admin API surface**: Added browse/delete endpoints for admin workflows:
  - `GET /memory`
  - `DELETE /memory/{id}`
- **Engine CLI web-admin flags**: Added `tandem-engine serve` flags:
  - `--web-ui`
  - `--web-ui-prefix`
- **Desktop Agent Command Center (first integration pass)**: Added an orchestrator-embedded command center UI for Agent Teams with live mission/instance status, spawn controls, and spawn-approval decision actions.
- **Agent-Team approval action endpoints**: Added explicit spawn approval decision routes:
  - `POST /agent-team/approvals/spawn/{id}/approve`
  - `POST /agent-team/approvals/spawn/{id}/deny`
- **Engine memory write/list tools**: Added `memory_store` and `memory_list` tools to `tandem-tools` so agents can persist and audit memory directly from engine tool calls.
- **Global memory opt-in support**: `memory_search` now supports `tier=global` when explicitly enabled via `allow_global=true` or `TANDEM_ENABLE_GLOBAL_MEMORY=1`.
- **Shared memory DB auto-wiring in engine**: `tandem-engine` now auto-configures `TANDEM_MEMORY_DB_PATH` to the shared Tandem `memory.sqlite` path when unset, aligning connected app/tool memory access by default.
- **Engine host runtime context contract**: Added shared `HostRuntimeContext` (`os`, `arch`, `shell_family`, `path_style`) in shared types/wire payloads and surfaced it through engine health/session/run metadata.
- **Server run-start environment observability**: `session.run.started` lifecycle events now include canonical engine environment metadata for cross-client parity.
- **MCP Automated Agents surface (Desktop)**: Added a dedicated `Agent Automation` page (robot-nav entry) for connector operations, scheduled routine wiring, and run triage separate from Command Center orchestration.
- **Mission Workshop (Desktop)**: Added in-page mission drafting assistance that converts plain-language goals into routine objective, success criteria, and suggested execution mode.
- **Ready-made automation templates**: Added starter templates for daily research, issue triage, and release reporting with built-in `webfetch_document` inclusion patterns.
- **Automation run observability UX**: Added per-run event rail chips (`Plan/Do/Verify/Approval/Blocked/Failed`), run filters (`All/Pending/Blocked/Failed`), and run details panel (timeline/reasons/outputs/artifacts).
- **Automation model routing controls**: Added provider/model selection and preset-based model routing for standalone + orchestrated automations, including role model hints for orchestrator/planner/worker/verifier/notifier.
- **Automation model-selection run events**: Added `routine.run.model_selected` emission so selected provider/model and selection source are visible in run streams.

### Changed

- **Headless config/env support**: Added config/env handling for web admin and channel settings (`TANDEM_WEB_UI`, `TANDEM_WEB_UI_PREFIX`, and channel env overlays).
- **Channel runtime wiring**: Wired channel listener lifecycle into server startup/reload flow with status publication events for admin visibility.
- **Security headers for embedded UI**: Added strict response headers/CSP for admin HTML responses.
- **Engine command docs**: Updated engine command reference to include new web-admin flags.
- **Desktop-Tauri agent-team bridge**: Added typed Tauri commands and frontend API wrappers for template/mission/instance/approval listing, spawn, and cancel/decision actions.
- **Startup navigation default**: Desktop now always opens in Chat view on startup (with a TODO for future starter/landing flow) instead of restoring Command Center directly.
- **Command Center observability layout**: Added inline run-scoped Console panel and elevated workspace file browser support in Command Center to improve live swarm debugging.
- **`memory_search` scope policy**: Reworked strict scope enforcement to allow controlled global search while preserving default isolation behavior unless global is explicitly enabled.
- **Engine memory docs/examples**: Expanded CLI and engine README docs with `memory_store`, `memory_list`, and global memory usage examples.
- **Engine prompt assembly (OS-aware)**: `tandem-core` now prepends a deterministic `[Execution Environment]` block (engine-detected OS/shell/path style) to model runs by default (`TANDEM_OS_AWARE_PROMPTS` toggle).
- **Canonical OS authority policy**: Runtime behavior now trusts engine-detected host environment as the source of truth rather than client-provided OS hints.
- **Engine health diagnostics**: `/global/health` now exposes `environment` metadata for troubleshooting and external clients.
- **Routines/automations API compatibility in sidecar bridge**: Desktop routine calls now gracefully fall back to legacy `/routines` endpoints when `/automations` returns `404`, enabling mixed-version app/engine operation.
- **Automation docs expansion**: Added setup and usage guidance for MCP automated agents, provider notes (Arcade/Composio), headless “just run” flow, model-routing examples, and release-readiness test checklist.

### Fixed

- **Command Center tool-arg hardening**: `read`/`write` tool calls now validate argument shape (`JSON object` + non-empty `path`) and fail fast with structured `INVALID_TOOL_ARGS` instead of retry loops.
- **Workspace/path error taxonomy**: Replaced broad Windows `os error 3` pause messaging with clearer classification (`WORKSPACE_NOT_FOUND`, path-not-found fail-fast) so runs stop/recover deterministically.
- **Orchestrator retry-loop suppression**: Invalid tool args and path-not-found failures now fail task attempts directly instead of repeatedly re-queuing.
- **Task session workspace pinning**: Child task sessions are now pinned to the orchestrator workspace path, with preflight checks before session creation.
- **Workspace switch propagation (CC-001)**: New runs now persist canonical `workspace_root` and inject explicit workspace/cwd into tool execution so file ops resolve against the selected Command Center workspace (not stale process CWD).
- **Workspace hot-switch engine invalidation (CC-001)**: Switching active project now invalidates stale in-memory orchestrator engines bound to other workspaces, preventing cross-workspace drift in subsequent runs.
- **Selected Run objective readability (CC-002)**: Added inline objective truncation with `Show more` / `Show less` toggle so large prompts no longer overwhelm the Selected Run panel by default.
- **Runs list status visibility (CC-003)**: Runs now surface status badges, start/end timestamps, and last-error snippets in Command Center for faster state triage without digging through logs.
- **Tool timeout resilience for file ops**: Increased `read`/`write` tool timeouts to reduce premature synthetic terminal errors on larger workspaces.
- **Tool history ID collision fix**: Tool execution IDs now include session/message/part context (not just `part_id`) to prevent cross-session overwrite/correlation drift.
- **Structured stream error codes**: Stream tool/session terminal events now carry optional `error_code` metadata for clearer diagnostics in orchestrator + UI.
- **Builder prompt guardrails**: Builder-agent prompt now explicitly requires valid JSON tool args and non-empty `path` for file tools.

- **Desktop channel token persistence across restart**: Fixed a vault-unlock/startup race where channel bot tokens (Telegram/Discord/Slack) could fail to rehydrate into sidecar env before restart, causing saved channel connections to appear unconfigured after engine/app restart.
- **Model/provider routing hardening**: Chat, queue, rewind, undo, and command-center/orchestrator dispatches now require explicit provider+model instead of silently falling back.
- **Model selection persistence**: Fixed picker drift by persisting `providers_config.selected_model` from both Chat and Command Center selectors.
- **Provider runtime model override**: Provider calls now honor per-request model overrides, preventing unintended fallback execution (for example `gpt-4o-mini` when another model is selected).
- **OpenRouter attribution**: Added consistent request attribution headers so calls are identified as Tandem instead of unknown source.
- **Memory startup self-heal**: Corrupted/incompatible vector DB state is now detected, backed up, and auto-recovered to prevent repeated startup failures (`chunks iter error` / SQL logic errors).
- **Command Center task state/UI correctness**: Fixed paused/failed run status mapping and disabled launch actions while a run is already active to prevent duplicate swarm starts.
- **Autonomous swarm permission flow**: Orchestrator/Command Center sessions now auto-allow shell tool permissions in autonomous mode (no manual approve gate for each call).
- **Shell-call robustness**: Empty shell invocations now fail fast with explicit `BASH_COMMAND_MISSING` instead of hanging until watchdog timeout.
- **Windows shell compatibility**: Added Windows translation for common Unix shell calls used by agents (`ls -la`, `find ... -type f -name ...`) to PowerShell equivalents.
- **Stream watchdog noise reduction**: Suppressed false stream-degraded watchdog events while tools are actively pending.
- **Failed task recovery in Command Center**: Added per-task retry support that re-queues failed tasks, clears stale task failure state, and unblocks dependent tasks without forcing full run restart.
- **Failed task diagnostics clarity**: Failed task cards now surface richer validator/error detail so failure causes are visible in-place instead of opaque `session.error` noise.
- **Engine tool memory path mismatch**: Fixed default `memory_search`/memory tool DB resolution in headless engine runs by setting the shared memory DB path at runtime when not already provided.
- **Memory tool test coverage**: Added/updated tests to validate global-memory opt-in gates and prevent accidental unrestricted global access.
- **Cross-platform shell execution path**: Non-Windows engine shell tool execution now uses POSIX shell (`sh -lc`) instead of hardcoded PowerShell fallback.
- **Windows shell mismatch loops**: Expanded Windows shell guardrails to translate common Unix patterns, block unsafe Unix-only commands with structured guidance, and emit `os_guardrail_applied`/`guardrail_reason` metadata.
- **OS mismatch retry suppression**: Engine loop now suppresses repeated identical shell calls after path/shell mismatch signatures and steers model/tool flow toward cross-platform tools (`read`, `glob`, `grep`).
- **OS mismatch error taxonomy**: Server dispatch now classifies common path/shell mismatch failures as `OS_MISMATCH` for clearer diagnostics.
- **Automation endpoint 404 startup noise**: Reduced desktop sidecar error loops/circuit-breaker churn when new automation routes are unavailable on older engines via endpoint fallback handling.
- **Automation model policy validation**: Hardened create/patch validation for `model_policy` shape (`default_model`, `role_models.*` with required `provider_id` + `model_id`) and explicit clear semantics (`model_policy: {}` on patch).

## [0.3.7] - 2026-02-18

### Changed

- **Complete Simplified Chinese overwrite**: Replaced and normalized Simplified Chinese copy across major app surfaces, including startup messaging, settings, About page, theme picker, provider cards, packs metadata, and skills guidance.
- **Localization completeness pass**: Converted remaining hardcoded English strings in key screens to i18n keys and filled missing `zh-CN` locale coverage.
- **Language UX polish**: Improved live language switching consistency and ensured parity-safe locale catalogs for `en`/`zh-CN`.

## [0.3.6] - 2026-02-18

### Fixed

- **TUI stale shared-engine attach**: TUI now checks connected engine version during startup and applies stale-policy gating before attaching to an existing shared engine.
- **Default self-healing startup**: Added `TANDEM_ENGINE_STALE_POLICY` with default `auto_replace` so stale engines are replaced by a fresh managed engine automatically.
- **Port collision recovery**: When default/shared engine port is occupied by a stale process, TUI now falls back to an available local port for managed startup.
- **Runtime visibility**: `/engine status` now reports required engine version, stale policy, and connection source (`shared-attached` vs `managed-local`) to make diagnosis deterministic.

## [0.3.5] - 2026-02-18

### Added

- **Agent Teams MVP in engine server**: Added mission/instance spawning foundations with shared server-side spawn policy enforcement, role edges, budget/cap controls, capability scopes, SKILL.md hashing, and structured agent-team SSE events for command-center style observability.
- **Agent Teams API/docs surface**: Added new guide docs for rollout, API/events, spawn policy, and protocol matrix updates; added crate-level READMEs for `tandem-ai` and `tandem-tui` with cargo-first usage.
- **Separate Registry Publish Workflow**: Added isolated GitHub Actions registry workflow (`publish-registries.yml`) for crates.io + npm publishing with dedicated triggers, environment gates, and dry-run support.
- **CI Publish Scripts**: Added CI-safe publish helpers for crates and npm (`scripts/publish-crates-ci.sh`, `scripts/publish-npm-ci.sh`, `scripts/publish-npm-ci.ps1`) with idempotent skip behavior for already-published versions.

### Changed

- **Publish chain hardening**: Reworked crate publish/version dependency chain so new crate releases resolve correctly in order, including updated crate versions for `tandem-memory`, `tandem-tools`, `tandem-core`, `tandem-server`, `tandem-tui`, and `tandem-ai`.
- **Release metadata bump**: Bumped app/wrapper versions to `0.3.5` across `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and npm wrapper package manifests.
- **npm Wrapper Bin Layout**: Updated `@frumu/tandem` and `@frumu/tandem-tui` wrappers to ship stable JS bin launchers (`bin/*.js`) and use `bin/native/*` for downloaded binaries, improving npm/pnpm shim generation reliability.
- **npm Wrapper Postinstall**: Simplified wrapper postinstall command to `node scripts/install.js` to avoid package-manager-specific env var path issues.
- **Release Docs**: Expanded release process docs to include the new separate registry publish flow.

### Fixed

- **Windows publish-verify blocker**: Removed reliance on `--no-verify` workaround path by making `tandem-memory` publish verification resilient without dragging problematic static-CRT tokenizer/embedder linkage into default verification builds.
- **Wrapper/crate docs mismatch**: Clarified npm wrapper READMEs and added proper crate README instructions to prevent `npm install` guidance from leaking into crate-level workflows.
- **Cross-platform Wrapper Install Reliability**: Fixed Windows/pnpm failures where bin shims were not created before postinstall and where postinstall path resolution could fail in some package-manager environments.
- **TUI Sidecar Engine Freshness**: TUI now detects stale cached/bundled sidecar engine binaries and auto-refreshes them based on version comparison, preventing old sidecar versions from being reused.

## [0.3.2] - 2026-02-17

### Fixed

- **TUI PIN startup flow**: Startup now correctly enters unlock mode whenever a vault key exists, preventing repeated create-PIN prompts across restarts.
- **TUI provider key onboarding**: First-run setup now opens when no provider keys exist in the unlocked keystore, so provider API key entry is no longer skipped.
- **TUI onboarding state logic**: Startup PIN detection and provider onboarding checks now use separate, correct signals (vault existence vs. configured keys).

## [0.3.1] - 2026-02-17

### Fixed

- **TUI Provider Gating**: TUI startup/provider catalog handling now excludes the fallback `local` provider from configured-provider checks, preventing chat/session flow from starting without a real provider setup.
- **Provider Setup UX Consistency**: Key/setup checks now consistently use sanitized provider catalogs so `/key test` and startup gating match wizard behavior.
- **Desktop StreamHub Restart Recovery**: Sidecar `/event` subscription failures during startup/restart are now treated as transient for circuit-breaker accounting, preventing repeated `503` retries from opening the breaker and delaying recovery.
- **Desktop StreamHub Error Noise**: StreamHub now classifies common sidecar-transition subscription failures as recovering state retries instead of emitting repeated hard error telemetry/log spam.

## [0.3.0] - 2026-02-17

### Added

- **Engine-Native Mission Runtime**: Added mission APIs (`POST /mission`, `GET /mission`, `GET /mission/{id}`, `POST /mission/{id}/event`) backed by shared orchestrator reducer state.
- **Shared Orchestrator Crate**: Introduced `crates/tandem-orchestrator` with reusable mission models (`MissionSpec`, `MissionState`, `WorkItem`, `MissionEvent`, `MissionCommand`) and reducer interface.
- **Default Mission Gates**: Added reviewer/tester reducer transitions with rework handling and completion signaling.
- **Shared Resources Blackboard**: Added revisioned shared resource store + APIs (`GET/PUT/PATCH/DELETE /resource/{*key}`, `GET /resource?prefix=...`) and SSE stream (`GET /resource/events`).
- **Status Indexer**: Added engine-derived run status indexing into shared resources (`run/{sessionID}/status`) from session/tool events.
- **Memory Governance APIs**: Added scoped memory endpoints (`POST /memory/put`, `POST /memory/promote`, `POST /memory/search`, `GET /memory/audit`) with capability checks and partition validation.
- **Tiered Memory Promotion Pipeline**: Added scrub + audit promotion flow for `session/project/team/curated` memory tiers with explicit promotion controls.
- **Routine Scheduler + APIs**: Added routine persistence/scheduler and APIs (`POST/GET /routines`, `PATCH/DELETE /routines/{id}`, `POST /routines/{id}/run_now`, `GET /routines/{id}/history`, `GET /routines/events`).
- **Routine Policy Gates**: Added external side-effect gates emitting lifecycle events (`routine.fired`, `routine.approval_required`, `routine.blocked`) with history states.
- **Desktop + TUI Mission/Routine Parity**: Added Desktop sidecar + TUI client command parity for mission/routine observe/control workflows.
- **TUI Composer Upgrade**: Added multiline composer state with cursor navigation, delete-forward, and native paste event handling in `tandem-tui`.
- **TUI Markdown/Stream Pipeline**: Added tandem-local markdown renderer + newline-gated stream collector with reducer-level tail-flush correctness tests.
- **TUI Long-Transcript Performance Harness**: Added virtualization parity tests and ignored benchmark target for large transcript render profiling.

### Security

- **Provider Secret Drift Fix**: Re-aligned engine auth flow to prevent provider API keys from being persisted via config patch APIs.
- **Runtime Auth Source**: `PUT /auth/{provider}` now applies provider keys to runtime-only engine config state and reloads providers without writing secrets to disk.
- **Config Secret Rejection**: `PATCH /config` and `PATCH /global/config` now reject secret-bearing fields (`api_key`, `apiKey`) with `400 CONFIG_SECRET_REJECTED`.
- **Response Redaction**: Config API responses continue to redact secret fields so provider credentials are never returned in plaintext API payloads.

### Changed

- **Platform Boundary Formalization**: Moved to engine-first orchestrator ownership for durable mission/resource/routine state, with Desktop/TUI as control-center clients.
- **Release Contract Stability**: Promoted mission and routine event families to stable SDK contracts after snapshot and client-parity verification.
- **Design Control Plane**: Added/standardized architecture control docs (`docs/design/*`) with linked workboard/progress/decisions workflow (`W-###`).
- **TUI Key Sync Transport**: TUI now syncs unlocked keystore credentials to engine runtime via `/auth/{provider}` instead of writing keys through `/config`.
- **Desktop Runtime Auth Sync**: Desktop now pushes provider credentials to sidecar runtime auth after sidecar start/restart, aligning with keystore-first secret handling.
- **Config Layers**: Added an in-memory runtime config layer for ephemeral provider auth material (merged into effective config, never persisted).
- **TUI Transcript Rendering**: Replaced `tui-markdown` usage with tandem-local `pulldown-cmark` rendering and virtualized transcript line materialization.
- **TUI Render Throughput**: Added bounded per-message render cache (message fingerprint + width keyed) to reduce repeated markdown/wrap work per frame.

### Fixed

- **Spec/API Drift**: Synced docs with implemented APIs (including routine delete and memory audit routes) and cleaned stale orchestrator roadmap claims from active specs.
- **Progress Tracking Integrity**: Repaired progress log table formatting and upgraded phase tracking to continue under `W-016+` control-plane flow.
- **Plaintext Key Persistence Gap**: Fixed a regression where provider API keys could end up in Tandem config files under `%APPDATA%/tandem` when clients used config patch flows.
- **OpenRouter Auth Regression After Scrub**: Fixed post-scrub provider failures by wiring runtime auth to provider resolution instead of relying on persisted config secrets.
- **Browser CORS for Engine API**: Added CORS support to engine HTTP routes so browser-based examples using `X-Tandem-Token` work with preflight requests.
- **TUI Stream Merge Regression**: Prevented regressive success/failure snapshots from overwriting richer locally-finalized assistant stream tails.

## [0.3.0-rc.2] - 2026-02-15

### Added

- **Core/Providers**: Added explicit support for `copilot` and `cohere` providers, and set `google/gemini-2.5-flash` as the default Gemini model.
- **Core Session Titles**: Added smart session titling logic in `tandem-core` to derive clean session names from user prompts.
- **TUI Guide**: Added a comprehensive `TANDEM_TUI_GUIDE.md` covering installation, navigation, and usage.
- **Tandem Guide Book**: Added a new mdbook-based guide in `guide/` for better documentation structure.
- **Engine CLI Concurrency**: Added `tandem-engine parallel --json ... --concurrency N` to run multiple prompt tasks concurrently from one CLI process.
- **Engine Communication Guide**: Added `docs/ENGINE_COMMUNICATION.md` documenting desktop/TUI <-> engine runtime contracts, run lifecycle, and SSE usage.
- **Engine CLI Guide Expansion**: Added a comprehensive `docs/ENGINE_CLI.md` with bash/WSL-first command examples, direct tool calls, and serve+API workflows.

- **TUI Key Setup Wizard**: Added interactive API-key setup flow when a selected provider is not configured.
- **TUI Error Recall Command**: Added `/last_error` to quickly print the most recent prompt/system failure message.
- **TUI Working Indicator**: Added active-agent working status/spinner visibility in chat footer and grid pane titles.
- **TUI Request Center**: Added pending-request modal (`Alt+R` / `/requests`) for permission approvals and interactive question replies.
- **Shared Permission Defaults**: Added centralized permission-default rule builder in `tandem-core` for desktop + TUI consistency.
- **Skills Discovery Expansion**: Added multi-root skills discovery in `tandem-skills` for project and ecosystem directories (`.tandem/skill`, `.tandem/skills`, `~/.tandem/skills`, `~/.agents/skills`, `~/.claude/skills`, plus appdata compatibility).
- **Agent Skill Activation Controls**: Added optional `skills` field to agent frontmatter/config so end users can scope which skills are active per agent.
- **Strict `memory_search` Tool**: Added a new `memory_search` tool in `tandem-tools` with strict scope enforcement (requires `session_id` and/or `project_id`, blocks global-tier searches, validates tier/scope combinations).
- **Embedding Health API Surface**: Added shared embedding health (`status` + `reason`) in memory runtime types and manager API for UI/event consumption.

### Changed

- **Frontend history**: Added debounce to history refresh to prevent excessive re-fetching.
- **TUI key bindings**: Improved TUI key handling and help interface.
- **Scripts**: Updated benchmarking and sidecar download scripts.
- **Engine Default Port**: Standardized default engine endpoint to `127.0.0.1:39731` (away from common frontend port `3000`) with env overrides (`TANDEM_ENGINE_PORT`, `TANDEM_ENGINE_HOST`, `TANDEM_ENGINE_URL`).
- **Desktop/TUI Endpoint Alignment**: Desktop sidecar + TUI now share centralized default port configuration and honor env overrides for connection/spawn behavior.
- **Engine Testing Docs**: Updated `ENGINE_TESTING.md` examples to use `tandem-ai` crate commands and the new default engine port.

- **Keystore Key Mapping**: TUI now normalizes legacy/local keystore key names (e.g. `openrouter_key`, `*_api_key`, `opencode_*_api_key`) to canonical provider IDs before syncing to engine config.
- **Keystore -> Engine Sync**: On connect, TUI now syncs unlocked local keystore provider keys into engine provider config to keep desktop/TUI auth behavior consistent.
- **TUI Startup Order**: Startup now performs engine bootstrap check/download before allowing transition to PIN entry, keeping users on the matrix/connect screen until engine install is ready.
- **TUI Download UX**: Engine download view now shows byte-based progress (downloaded/total) with install phase text and last-error status on failures.
- **Transcript Rendering**: Chat flow renderer now wraps long lines for readable error/output text in narrow terminals.
- **Windows Dev Docs**: Expanded `ENGINE_TESTING.md` with PowerShell-safe build/copy/run commands and bash-vs-PowerShell clarity.
- **TUI Keybinds/UX**: Grid toggle moved to `Alt+G`; request center added on `Alt+R`; scroll speed increased for line/page scrolling.
- **Startup + PIN UX**: PIN prompt re-centered for fullscreen, digit-only PIN input enforced, and connecting screen now stays active until engine readiness checks complete.
- **Markdown Pipeline**: TUI transcript path now uses `tui-markdown` preprocessing for assistant markdown content.
- **Mode Tool Gating**: `skill` is now universal at mode level (not blocked by mode `allowed_tools` allowlists).
- **Skill Tool Scope Behavior**: `skill` now respects agent-level equipped skill lists when present (filtered list/load), while still being callable from all modes.
- **Memory Crate Integration**: `src-tauri` now consumes `crates/tandem-memory` directly via re-export facade and removed duplicated local memory implementation files.
- **Memory/Embedding UX Telemetry**: `memory_retrieval` stream events and memory settings now include embedding health fields (`embedding_status`, `embedding_reason`), and chat/settings UI surface those states.
- **Memory Telemetry Persistence**: Memory lifecycle events are now persisted into tool history (`memory.lookup`, `memory.store`) so session reloads can rehydrate memory badges and console entries.
- **Chat Memory Badge Reliability**: Chat now buffers memory telemetry that can arrive before assistant content chunks and deterministically attaches it to the next assistant message.

### Fixed

- **Desktop Permission Routing**: Permission prompts now stay scoped to the active session, preventing cross-session/parallel-client approval leakage into the wrong chat.
- **Question Overlay Recovery**: Desktop now normalizes `permission(tool=question)` into question-request UI flow so walkthrough prompts consistently appear in the modal.
- **Plan Mode Todo Reliability**: Fixed repeated `todo_write` no-op loops (`0 items`) by normalizing common todo payload aliases (`tasks`, `items`, checklist/text forms) and skipping empty todo calls.
- **Plan Mode Clarification Flow**: When planning cannot produce concrete todos, the engine now emits a structured `question.asked` fallback instead of silently proceeding with prose-only clarification.
- **Todo Fallback Precision**: Todo extraction fallback now ignores plain prose and only derives tasks from structured checklist/numbered lines, preventing accidental phantom tasks.
- **Silent Prompt Failures in TUI**: Stream failures are now surfaced from runtime events (`session.error` and failed `session.run.finished`) instead of appearing as no-response hangs.
- **Run Stream Stability**: Fixed run-scoped SSE handling so unrelated events do not prematurely terminate active stream processing.
- **No-Response Completion Case**: Added explicit fallback error messaging when a run completes without streamed deltas or assistant content.
- **Provider Auth in TUI**: Fixed a key-discovery mismatch that prevented existing desktop-stored keys from being recognized by TUI.
- **Engine Download Retry in TUI**: Download failures no longer lock out retries for the remainder of the process; retry/backoff now allows recovery without restarting TUI.
- **Debug Engine Bootstrap Fallback**: Debug builds now fall back to release download when no local dev engine binary is found.
- **Keystore Corruption Routing**: Unreadable/corrupt keystore files now route to create/recovery behavior instead of trapping users in unlock failures.
- **Request Visibility**: Replaced noisy in-transcript request activity lines with dedicated request/status UI.
- **Permission Clarity in Plan Mode**: Request modal now shows mode/tool context and explains why permission is requested, including `tool: question` previews.
- **Question Handling**: Added custom-answer support alongside multiple-choice options and fixed `permission(tool=question)` normalization into question-answer flow.
- **Skills Discovery Determinism**: Duplicate skill names are now resolved by deterministic priority (project roots override global roots).
- **Windows `tandem-memory` Test Linking**: Fixed MSVC CRT mismatch (`LNK2038`, `MT_StaticRelease` vs `MD_DynamicRelease`) by vendoring/patching `esaxx-rs` to avoid static CRT linkage in this workspace.
- **Corrupt Memory DB Startup Recovery**: Added SQLite integrity validation (`PRAGMA quick_check`) at memory DB startup so malformed databases are detected and auto-backed-up/reset before runtime writes.
- **Session Rehydration Gaps**: Fixed missing memory retrieval/storage telemetry after reload by rehydrating persisted memory rows into assistant message badges and console history.
- **Idle Stream Health**: Stream watchdog no longer marks the desktop stream as degraded when idle without active runs or tool calls.

## [0.3.0-rc.1] - 2026-02-14

### Added

- **Web Markdown Extraction**: Added `webfetch_document` to convert HTML into clean Markdown with links, metadata, and size stats.
- **Tool Debugging**: Added `mcp_debug` to capture raw MCP responses (status, headers, body, truncation).
- **CLI Tool Runner**: Added `tandem-engine tool --json` to invoke tools directly from the engine binary.
- **Runtime API Tests**: Added sidecar and TUI coverage for conflict handling, recovery, and cancel-by-runID flows.

### Changed

- **Web Tool Defaults**: Default modes now include `websearch` and `webfetch_document` (approval gated).
- **Tool Permissions**: Added permission support for `webfetch_document` in mode rules.
- **MCP Accept Header**: MCP calls now accept `text/event-stream` responses for SSE endpoints.
- **Runtime API Naming**: Renamed `send_message_streaming` and related client/sidecar identifiers to split-semantics naming.
- **Rust SDK/Runtime Licensing**: Rust packages now ship under `MIT OR Apache-2.0` (app/web licensing unchanged).
- **Tool Arg Integrity Pipeline**: Added normalized tool-arg handling across permission + execution boundaries with websearch query-source tracking.
- **Dev Sidecar Build Handshake**: Added `/global/health` build metadata (`build_id`, `binary_path`) and desktop mismatch diagnostics.

### Docs

- **Engine Testing Guide**: Added tool testing workflows, size savings example, and Windows quickstart for tauri dev.

### Fixed

- **Websearch Empty-Args Failures**: Prevented repeated `websearch` executions with `{}` by recovering/inferencing queries and emitting explicit terminal errors when unrecoverable.
- **Websearch Looping**: Added read-only dedupe and loop-guard controls for repeated identical `websearch` calls.
- **Provider Auth Error Clarity**: Provider auth failures now emit provider-specific API key guidance (for example OpenRouter key hints).
- **Desktop External Links**: Markdown links in assistant messages now open reliably through Tauri opener integration.

## [0.2.25] - 2026-02-12

### Added

- **Canonical Marketing Skills (Core 9)**: Added starter skill templates for `product-marketing-context`, `content-strategy`, `seo-audit`, `social-content`, `copywriting`, `copy-editing`, `email-sequence`, `competitor-alternatives`, and `launch-strategy`.
- **Marketing Skills Canonical Map**: Added `docs/marketing_skill_canonical_map.md` to document no-duplicate routing and fallback strategy.

### Changed

- **Skill Template Install Behavior**: `skills_install_template` now installs the full template directory recursively (not just `SKILL.md`), so bundled `references/`, scripts, and assets ship with installs.
- **Marketing Starter Ordering**: Updated `SkillsPanel` recommendations to prioritize canonical marketing skills over legacy/fallback templates for marketing-intent discovery.
- **Shared Marketing Context Path**: Replaced `.claude/product-marketing-context.md` references with `scripts/marketing/_shared/product-marketing-context.md` and included shared context template references.

### Fixed

- **Skill Template Parsing Reliability**: Re-saved template `SKILL.md` files in UTF-8 without BOM to prevent false `missing or malformed frontmatter (---...---)` parser failures.
- **Template Frontmatter YAML**: Fixed invalid `tags` format in `development-estimation` and `mode-builder` (`string` -> YAML sequence).
- **Legacy Marketing Template Labeling**: Updated overlapping bundled marketing templates to clearly indicate legacy/fallback usage.
- **Version Metadata Sync**: Bumped version to `0.2.25` across app metadata for release consistency.

## [0.2.24] - 2026-02-12

### Added

- **Custom Modes (Phased MVP Complete)**: Added end-to-end custom mode support with backend-authoritative enforcement, including mode listing, create/update/delete, import/export, deterministic precedence (`builtin < user < project`), and safe fallback behavior.
- **Guided Mode Builder**: Added a non-technical, step-by-step mode creation wizard in `Extensions -> Modes`.
- **Mode Management in Extensions**: Added a dedicated `Modes` area under `Extensions` with `Guided Builder` and `Advanced Editor` views.
- **AI-Assisted Mode Builder**: Added optional AI assist flow in Guided Builder with:
  - `Start AI Builder Chat`
  - paste-and-parse JSON preview before apply
  - new bundled skill template: `mode-builder`
- **Mode Icons**: Added selectable mode icons that render in chat mode selector UI.

### Changed

- **Chat Mode Selector**: Mode selector now loads built-in + custom modes dynamically and uses compact descriptions for custom entries.
- **Memory Indexing Default**: `auto_index_on_project_load` now defaults to `true` for new users/devices.

### Fixed

- **Version Metadata Sync**: Updated `tauri.conf.json`, `package.json`, and `Cargo.toml` so auto-updates detect new releases correctly.

## [0.2.23] - 2026-02-12

### Added

- **Global Activity Indicators**: Added top-right runtime badges for concurrent background work (`CHATTING` and `ORCHESTRATING` counts) so active work remains visible while navigating between sessions/views.
- **Session List Running State UX**: Added explicit running status indicators in the Sessions sidebar for active chat sessions and orchestrator runs.
- **Orchestrator Budget Controls**: Added in-panel budget actions so users can extend run limits (`Add Budget Headroom`) or relax caps for long-running orchestrations (`Relax Max Caps`) without starting over.

### Changed

- **Session Selection Behavior**: Selecting a normal chat session now exits Orchestrator panel mode and clears stale selected run context.
- **Sidebar Status Presentation**: Refined running indicators to avoid duplicate spinners and keep status signal in a consistent location (`RUNNING` on the metadata line).
- **Chat Activity Accounting**: Chat running counts now derive from global sidecar stream events (session-scoped), not only the currently mounted chat component state.

### Fixed

- **Orchestrator Console Persistence**: Fixed orchestrator Console tab history clearing on drawer reopen by scoping logs to run sessions and loading persisted tool events across base + task child sessions.
- **Orchestrator Console Live Scope**: Fixed Console stream bleed by filtering live tool events to only the orchestrator run's related session IDs.
- **Orchestrator Retry Error Visibility**: Fixed retry/restart failures being visible only in logs by surfacing run failure reasons directly in Orchestrator UI alerts.
- **Orchestrator Failure Context**: Improved terminal failure messaging to include concrete failed-task error details (e.g. provider/model-not-found) instead of generic max-retry text.
- **Orchestrator Budget Recovery**: Fixed budget-limit dead ends by allowing failed budget runs to move back to resumable state after caps are increased.
- **Concurrent Chat Session Indicators**: Fixed sidebar/chat-header indicators dropping when switching selection by tracking running sessions globally and rendering status per session ID.
- **Budget Warning Log Spam**: Throttled repetitive orchestrator budget warning logs (e.g. `wall_time at 80%`) to log on meaningful threshold progression/cooldown instead of every loop tick.

## [0.2.22] - 2026-02-11

### Fixed

- **Orchestrator Run Isolation by Project**: Prevented Orchestrator mode from reusing a stale run across projects by clearing selected run state when switching/adding projects and scoping run selection to the active workspace.
- **Orchestrator Auto-Resume Behavior**: Opening Orchestrator with no explicit run now auto-resumes only active runs (`planning`, `awaiting_approval`, `executing`, `paused`) instead of reopening terminal/completed history by default.

## [0.2.21] - 2026-02-11

### Added

- **Model Selector Provider Filter**: Added an explicit provider selector inside the chat model dropdown (`All` + visible providers) so users can narrow large catalogs without horizontal scrolling.
- **Provider-Aware Search Token**: Added `provider:<id-or-name>` support in model search (for example `provider:openrouter sonnet`) to quickly scope results from the keyboard.

### Changed

- **Model Selector UX**: Replaced horizontal provider chips with a compact full-width provider dropdown for better scalability with many providers.
- **Model Selector Clarity**: Added helper copy ("Showing configured providers + local") to explain why some providers are hidden by default.
- **Provider Filter Behavior**: Provider filters now reset safely to `All` when a previously selected provider is no longer available after model reload.

### Fixed

- **Provider-Scoped Empty State**: Empty states in model selection now explain when no matches exist for the active provider filter.
- **Fullscreen File Preview Readability**: Increased fullscreen preview opacity and backdrop strength so file content remains readable on highly transparent/gradient themes (e.g. Pink Pony) instead of blending into the app background.

## [0.2.20] - 2026-02-11

### Added

- **Sidecar Update Compatibility Metadata**: Sidecar status now exposes `latestOverallVersion` and `compatibilityMessage` so the UI can clearly explain when newest overall and newest compatible releases differ.
- **Global Stream Hub**: Added a single long-lived sidecar stream substrate (`stream_hub`) that fans out events to chat, orchestrator, and Ralph, reducing duplicate subscriptions and race-prone stream wiring.
- **Event Envelope v2 (Additive)**: Added `sidecar_event_v2` with envelope metadata (`event_id`, `correlation_id`, `ts_ms`, `session_id`, `source`, `payload`) while keeping legacy `sidecar_event` for compatibility.
- **Stream Health Signaling**: Added explicit stream health events (`healthy`, `degraded`, `recovering`) emitted from the backend and surfaced in chat UI.
- **Chat Message Queue IPC**: Added queue APIs for busy-agent workflows: `queue_message`, `queue_list`, `queue_remove`, `queue_send_next`, `queue_send_all`.
- **Skills Import Preview + Conflict Policies**: Added `skills_import_preview` and `skills_import` with deterministic conflict strategies: `skip`, `overwrite`, `rename`.
- **Skills Pack/Zip Import Support**: Added multi-skill zip import parsing (`SKILL.md` discovery) with pre-apply preview summary.
- **Richer Skill Metadata Surface**: Expanded skill metadata handling to include `version`, `author`, `tags`, `requires`, `compatibility`, and `triggers`.

### Fixed

- **OpenCode Sidecar Release Discovery**: Sidecar update checks now query GitHub Releases with pagination (`per_page=20`, multi-page) instead of relying on a single latest path.
- **Update Target Selection**: Sidecar updater now selects the newest compatible release for the current platform/architecture by filtering assets from release metadata and skipping drafts (and prereleases unless beta channel is enabled).
- **Rate Limit Resilience**: Added conditional GitHub requests (`If-None-Match`, `If-Modified-Since`), local release-cache reuse, and check debouncing to reduce API pressure and improve reliability when offline/rate-limited.
- **Version Comparison Correctness**: Updater now uses semantic version comparison (with fallback parsing) to prevent incorrect update prompts from string-based version checks.
- **Sidecar Update Messaging**: Improved update overlay messaging to surface compatibility context instead of always presenting newest-tag text.
- **Console History Persistence**: Fixed historical tool executions not loading in the Console tab by correctly parsing persisted `type: "tool"` messages (which differ from live streaming format) and simplifying part-ID resolution.
- **Chat Jump Button**: Fixed "Jump to latest" button floating in the middle of the view by positioning it as an absolute overlay at the bottom of the message area, independent of scroll content height.
- **Streaming Subscription Duplication**: Eliminated per-request stream subscription in `send_message_streaming`; message streaming now uses shared stream bus events, reducing duplicate event emission risks.
- **Memory Retrieval Event Handling in Chat**: Wired frontend handling for `memory_retrieval` stream events so retrieval telemetry is now visible in the active chat flow.
- **Orchestrator/Ralph Stream Contention**: Migrated orchestrator and Ralph loop event consumption to stream-hub fanout instead of opening independent sidecar event feeds.
- **Chat Event Duplication Under Load**: Added deterministic frontend dedupe keyed by `event_id` for v2 stream envelopes.

### Changed

- **Streaming Architecture**: Shifted Tandem to a hub-first streaming model with additive v2 envelopes and backward-compatible legacy event emission during migration.
- **Chat UX During Generation**: Pressing Enter while generation is active now queues messages (FIFO) with inline queue controls for send-next/send-all/remove.
- **Tool Activity Presentation**: Updated inline assistant tool summary to show compact process-oriented status (step count, running/pending/failed counts, duration) with detail drill-down retained.

## [0.2.19] - 2026-02-11

### Added

- **Memory Retrieval Telemetry**: Chat requests now run memory retrieval before sending prompts, emit a `memory_retrieval` stream event, and include balanced telemetry (usage, chunk counts, latency, score range, short query hash) without logging raw query text or chunk contents.
- **Chat Memory Badge**: Assistant responses now show a memory capsule with a brain icon and retrieval status (used/not used, chunks, latency) for verifiable retrieval visibility per response.
- **Console Tab (Logs Drawer)**: Added a dedicated Console tab for tool-execution events and approvals in the Logs drawer workflow.

### Fixed

- **Memory Retrieval Coverage**: Wired retrieval context injection into both `send_message` and `send_message_streaming` so normal chat requests can actually use indexed vector memory.
- **Sidecar Duplicate Spawn Race**: Prevented duplicate OpenCode/Bun sidecar launches by serializing sidecar start/stop lifecycle transitions with a lifecycle lock.
- **Logs Drawer Fullscreen Height**: Fixed logs panel sizing so height is fully dynamic in fullscreen instead of staying at the smaller constrained height.
- **Logs Redundancy**: Removed the redundant OpenCode sidecar log tab from the logs viewer and consolidated command activity under the Console tab.
- **Pink Pony Readability**: Tuned Pink Pony theme contrast, surface opacity, borders, and text colors to improve legibility on bright backgrounds.
- **Chat Performance**: Significantly improved rendering performance for long chat sessions by implementing list virtualization and component memoization.
- **Production Build**: Fixed a TypeScript error in the Logs Drawer component (`ResizeObserver` type mismatch) that was blocking production builds.

### Changed

- **Memory Log Signal**: Memory retrieval logging now uses a distinct `tandem.memory` target and a brain marker for easier scanning in logs.
- **Production Frontend Build**: Production Vite builds now drop `console.*` and `debugger` statements.

## [0.2.18] - 2026-02-10

### Added

- **Python**: Auto-open the Python Setup (Workspace Venv) wizard when Python is blocked by venv-only policy enforcement (helps LLM-triggered Python attempts recover quickly).
- **Python**: Extend venv-only enforcement to staged/batch execution (preflight staged operations before approving any tool calls).
- **Python**: Add a shared policy helper + tests for consistent enforcement across approval paths.
- **Packs (Python)**: Add `requirements.txt` to the Data Visualization and Finance Analysis packs; update their docs to install via the workspace venv.
- **Packs**: Install pack-level `CONTRIBUTING.md` when present (copied alongside `START_HERE.md`).
- **Files**: Add a dock mount + fullscreen toggle for file previews.

### Fixed

- **Skills/Templates**: Fix bundled starter skill templates with missing YAML frontmatter fields so they no longer get skipped on startup.
- **Python**: Improve the requirements install UX by defaulting to the workspace and auto-detecting `requirements*.txt` when present.

### Known Issues

- **Files Auto-Refresh (WIP)**: The Files tree does not reliably refresh when tools/AI create new files in the workspace. Deeper investigation needed; workaround is to navigate away and back to Files.

## [0.2.17] - 2026-02-10

### Fixed

- **Custom Background Opacity Slider (Packaged Builds)**: Fix opacity changes causing the background image to flash or disappear in bundled builds by keeping the resolved image URL stable and updating only opacity.
- **Background Layering**: Render the custom background image as a dedicated fixed layer so it consistently appears across views without impacting overlay layout.

## [0.2.16] - 2026-02-10

### Fixed

- **Update Overlay Layout**: Fix the in-app update prompt becoming constrained/squished due to theme background layering CSS.

## [0.2.15] - 2026-02-10

### Fixed

- **Custom Background Image Loading (Packaged Builds)**: Fix custom background images failing to load after updating by falling back to an in-memory `data:` URL when the `asset:` URL fails.

## [0.2.14] - 2026-02-10

### Added

- **Themes: Background Art Pass**: Add richer background art for Cosmic Glass (starfield + galaxy glow), Pink Pony (thick arcing rainbow), and Zen Dusk (minimalist ink + sage haze).
- **Theme Background Support**: Add an `app-background` utility class so gradient theme backgrounds render correctly throughout the app (not just as a solid `background-color`).
- **Custom Background Image Overlay**: Allow users to choose a background image (copied into app data) and overlay it on top of the active theme, with an opacity slider in Settings.
- **File Text Extraction (Rust)**: Add best-effort, cross-platform text extraction for common document formats (PDF, DOCX, PPTX, XLSX/XLS/ODS/XLSB, RTF) via the `read_file_text` command so attachments can be used by skills without requiring Python.
- **Python Workspace Venv Wizard**: Add a cross-platform in-app Python setup wizard to create a workspace-scoped venv at `.opencode/.venv` and install dependencies into it (never global).
- **Docs: Theme Contribution Guide**: Add guidance for creating and iterating on theme backgrounds.

### Fixed

- **Settings/About/Extensions Navigation**: Restore Settings/About/Extensions panels after a regression where these views would not appear.
- **Overlay Layering**: Ensure theme/background layers render consistently across main views (chat + settings) without unintended translucency.
- **Startup Session Restore**: Fix restored sessions appearing selected but not opening until reselecting the folder (defer history load until the sidecar is running; allow re-clicking the selected session to reload).

### Changed

- **Packs UI**: Style runtime requirement pills consistently.

## [0.2.13] - 2026-02-10

### Added

- **Skill Templates: New Starter Skills**: Add two new bundled starter skills: `brainstorming` and `development-estimation`.
- **Skill Templates: Runtime Pills**: Starter skill cards now show optional runtime hints (e.g. Python/Node/Bash) via `requires: [...]` YAML frontmatter.
- **Skills UI: Installed Skill Discoverability**: Add clearer install/manage UX (runtime note, counts for folder vs global installs, and a jump-to-installed action).

### Fixed

- **Dev Skill Template Discovery**: In `tauri dev`, load starter skill templates from `src-tauri/resources/skill-templates/` so newly added templates appear immediately (avoids stale `target/**/resources/**` copies).
- **Logs Viewer UX**: Improve log viewer usability (fullscreen mode, and copy feedback).
- **Skill Template Parsing**: Fix invalid bundled skill template frontmatter (missing `name`) so it is not skipped.

### Changed

- **Packs UI**: Show packs only (remove starter skills section) and move the runtime note to the top of the Packs page.
- **Docs**: Expand contributor documentation with a developer guide for adding skills.

## [0.2.12] - 2026-02-09

### Fixed

- **Orchestrator Model Routing**: Persist the selected provider/model on orchestrator runs and prefer it when sending prompts so runs don't start with an "unknown" model or send messages without an explicit model spec.
- **Orchestrator Restart/Retries**: Prevent "restart" from instantly reporting success without doing any work (guard against empty plans; allow restarting completed runs to rerun the full plan).
- **Logs Viewer Copy/Scroll**: Make long log lines easy to inspect and share (horizontal scroll + selected-line preview + copy helpers).
- **Orchestrator Run Deletion**: Allow deleting orchestrator runs from the Sessions sidebar (removes the run from disk and deletes its backing OpenCode session).
- **Release to Discord**: Automated releases now post to Discord via the release workflow (release:published events triggered by `GITHUB_TOKEN` are not delivered to other workflows).
- **Release to Discord**: Ensure Discord notifications fire for automated releases by posting from the release workflow (instead of relying on `release: published`, which doesn't trigger when publishing via `GITHUB_TOKEN`).

## [0.2.11] - 2026-02-09

### Added

- **On-Demand Logs Viewer**: Add a right-side Logs drawer that can tail Tandem app log files (from the app data `logs/` directory) and show OpenCode sidecar stdout/stderr (captured into a bounded in-memory ring buffer). Streaming only runs while the drawer is open/active to avoid baseline performance cost.
- **Poe Provider**: Add Poe as an OpenAI-compatible provider option (endpoint + `POE_API_KEY`). Thanks [@CamNoob](https://github.com/CamNoob).

### Fixed

- **OpenCode Session Hangs**: Prevent sessions from getting stuck indefinitely when a tool invocation never reaches a terminal state by recognizing more terminal tool statuses, ignoring heartbeat/diff noise in the stream, and fail-fast cancelling with a surfaced error after a timeout.
- **Sidecar StdIO Deadlock Risk**: Always drain the OpenCode sidecar stdout/stderr pipes so the sidecar cannot block if it emits high-volume output.
- **Log Noise Reduction**: Ignore OpenCode `server.*` heartbeat SSE events (and downgrade other unknown SSE events) to prevent log spam during long-running sessions.
- **Vault Locked Log Spam**: Avoid warning-level logs when the keystore isn't available because the vault is locked (expected state).
- **Release Pipeline Resilience**: Retry GitHub Release asset uploads to reduce flakes during transient GitHub errors.

## [0.2.10] - 2026-02-09 (Failed Release)

- Release attempt failed due to GitHub release asset upload errors during a GitHub incident; no assets were published. v0.2.11 re-cuts the same changes.

## [0.2.9] - 2026-02-09

### Added

- **Project File Indexing**: Add an incremental, per-project file index for workspace embeddings with total/percent progress reporting.
- **Memory Stats Scope**: Switch Vector Database Stats between All Projects and Active Project views.
- **Auto-Index Toggle**: Optionally auto-index the active project on load (with a short cooldown).
- **Clear File Index**: Clear only file-derived vectors/chunks for a project (optional VACUUM) to reclaim space.

### Fixed

- **Question Prompts**: Properly handle OpenCode `question.asked` events (including multi-question requests) and render an interactive one-at-a-time wizard with multiple-choice + custom answers; replies are sent via the OpenCode `/question/{requestID}/reply` API.
- **Startup Session History**: When restoring the last active project on launch, automatically load its sessions by scoping OpenCode `/session` and `/project` listing calls to the active workspace directory.
- **Windows Dev Reload Sidecar Cleanup**: Prevent orphaned OpenCode sidecar (and Bun) processes when the app is restarted during `tauri dev` rebuilds by attaching the sidecar to a Windows Job Object (kill-on-close).

## [0.2.8] - 2026-02-09

### Added

- **Multi Custom Providers (OpenCode)**: Support selecting any provider from the OpenCode sidecar catalog (including user-defined providers by name in `.opencode/config.json`), not just the built-in set.

### Fixed

- **Model Selection Routing**: Persist the selected `provider_id` + `model_id` and prefer it when sending messages, so switching to non-standard providers actually takes effect.

## [0.2.7] - 2026-02-08

### Fixed

- **OpenCode Config Safety**: Prevent OpenCode config writes from deleting an existing `opencode.json` when replacement fails (e.g. file locked on Windows).
- **Sidecar Idle Memory**: Set Bun/JSC memory env hints to reduce excessive idle memory usage.

## [0.2.6] - 2026-02-08

### Fixed

- **macOS Release Builds**: Disabled codesigning/notarization by default in the release workflow to prevent macOS builds from failing when Apple certificate secrets are missing or misconfigured. (Enable with `MACOS_SIGNING_ENABLED=true` repo variable.)

## [0.2.5] - 2026-02-08

### Fixed

- **Release Build Trigger**: Bumped version/tag to ensure GitHub Releases builds run with the corrected workflow configuration.

## [0.2.4] - 2026-02-08

### Added

- **Vector DB Stats (Settings)**: Added a Memory section in Settings to view vector database stats and manually index the current workspace.
- **macOS Release Verification**: Release/CI now includes Gatekeeper checks (`codesign`, `spctl`, `stapler validate`) for produced DMGs (informational unless Apple signing secrets are configured).

### Fixed

- **Starter Pack Installs (Windows/macOS/Linux)**: Fixed pack/template resolution in packaged builds so Starter Packs and Starter Skills can be installed correctly from bundled resources.
- **Onboarding For Custom Providers**: Custom providers (e.g. LM Studio / OpenAI-compatible endpoints) are now treated as “configured”, preventing onboarding from forcing users back to Settings.
- **Pack Install Errors**: Pack install failures now surface the underlying error message in the UI.

## [0.2.3] - 2026-02-08

### Fixed

- **Orchestration Session Spam**: Orchestration no longer creates endless new root chat sessions during execution.
  - Sub-agent/task sessions are now created as child sessions (so they don't flood the main session list).
  - Session listing now prefers root sessions only, with a fallback for older sidecars.

## [0.2.2] - 2026-02-08

### Added

- **Knowledge Work Skills Migration**: Completed the migration of all legacy knowledge work skills to the Tandem format.
  - **Productivity Pack**: `productivity-memory`, `productivity-tasks`, `productivity-start`, `productivity-update`, `inbox-triage`, `meeting-notes`, `research-synthesis`, `writing-polish`.
  - **Sales Pack**: `sales-account-research`, `sales-call-prep`, `sales-competitive-intelligence`, `sales-create-asset`, `sales-daily-briefing`, `sales-draft-outreach`.
  - **Bio-Informatics Pack**: `bio-instrument-data`, `bio-nextflow-manager`, `bio-research-strategy`, `bio-single-cell`, `bio-strategy`.
  - **Data Science Pack**: `data-analyze`, `data-build-dashboard`, `data-create-viz`, `data-explore-data`, `data-validate`, `data-write-query`.
  - **Enterprise Knowledge Pack**: `enterprise-knowledge-synthesis`, `enterprise-search-knowledge`, `enterprise-search-source`, `enterprise-search-strategy`, `enterprise-source-management`.
  - **Finance Pack**: `finance-income-statement`, `finance-journal-entry`, `finance-reconciliation`, `finance-sox-testing`, `finance-variance-analysis`.
  - **Legal Pack**: `legal-canned-responses`, `legal-compliance`, `legal-contract-review`, `legal-meeting-briefing`, `legal-nda-triage`, `legal-risk-assessment`.
  - **Marketing Pack**: `marketing-brand-voice`, `marketing-campaign-planning`, `marketing-competitive-analysis`, `marketing-content-creation`, `marketing-performance-analytics`.
  - **Product Pack**: `product-competitive-analysis`, `product-feature-spec`, `product-metrics`, `product-roadmap`, `product-stakeholder-comms`, `product-user-research`.
  - **Support Pack**: `support-customer-research`, `support-escalation`, `support-knowledge-management`, `support-response-drafting`, `support-ticket-triage`.
  - **Design & Frontend Pack**: `canvas-design`, `theme-factory`, `frontend-design`, `web-artifacts-builder`, `algorithmic-art`.
  - **Internal Comms**: `internal-comms`.
  - **Utilities**: `cowork-mcp-config-assistant`.
- **Skill Templates**: All migrated skills are now available as offline-compatible templates in the `src-tauri/resources/skill-templates` directory.
- **Brand Neutralization**: All skills have been updated to be model-agnostic, removing dependencies on specific AI providers.
- **Extensions**: New top-level Extensions area with tabs for Skills, Plugins, and Integrations (MCP).
- **MCP Integrations UI**: Add/remove remote HTTP and local stdio MCP servers with scope support (Global vs Folder).
- **MCP Presets**: Added popular remote presets (including Context7 and DeepWiki) for quick setup.
- **Skills Search**: Added a search box to filter both Starter skills and Installed skills.
- **New Skill Template**: `youtube-scriptwriter` starter skill template.

### Improved

- **MCP Test Connection**: Test now performs a protocol-correct MCP `initialize` POST and validates JSON-RPC (including SSE responses) instead of using HEAD/GET.
- **MCP Status UX**: More accurate status mapping and actionable error messages (auth required, wrong URL, incompatible transport, deprecated endpoint).

### Fixed

- MCP connection tests no longer report "Connected" for non-2xx HTTP responses like 405/410.

## [0.2.1] - 2026-02-07

### Added

- **Guided onboarding wizard** to drive a first outcome (choose folder → connect AI → run starter workflow).
- **Starter Packs**: bundled, offline workflow packs you can install into a folder from inside the app.
- **Starter Skills gallery**: bundled, offline skill templates with an “Advanced: paste SKILL.md” option retained.
- **Contributor hygiene**: GitHub issue/PR templates and new product/architecture docs at repo root.

### Improved

- **Orchestration reliability**:
  - Increased default budgets (iterations/sub-agent runs) and auto-upgraded legacy runs with too-low limits.
  - Provider rate-limit/quota errors now **pause** runs (instead of burning retries) so you can switch model/provider and resume.
- **Provider switching**: fixed stale env var propagation by explicitly syncing/removing provider API key env vars and restarting sidecar when provider toggles change.
- **CI confidence**: frontend lint now fails the build instead of being ignored.

### Fixed

- Orchestrator could “explode” sub-agent runs due to tasks not being marked finished on error (leading to endless requeue/recovery loops).
- Model/provider could not be changed after a run failed; model selection is now available to recover and resume.

## [0.2.0] - 2026-02-06

### Added

- **Multi-Agent Orchestration**: Introduced a major new mode for complex task execution.
  - **Task DAG**: Supports dependency-aware task graphs (Planner -> Builder -> Validator).
  - **Sub-Agents**: Orchestrates specialized agents for planning, coding, and verifying.
  - **Cost & Safety**: Implements strict budget controls (tokens, time, iterations) and policy-based tool gating.
  - **Visualize**: New Kanban board and budget meter to track progress in real-time.
- **Unified Session Sidebar**: Completely redesigned the sidebar to merge chat sessions and orchestrator runs into a single, cohesive chronological list.
  - **Project Grouping**: Items are smartly grouped by project with sticky headers.
  - **Status Indicators**: Orchestrator runs show live status (Running, Completed, Failed).

## [0.1.15] - 2026-02-03

### Added

- **Unified Update UI**: Replaced the disparate update experiences for OpenCode (Sidecar) and Tandem (App) with a single, polished, full-screen overlay component.
- **Conflict Resolution**: The new `AppUpdateOverlay` takes precedence over other update screens, ensuring that app updates (which restart the application) are handled cleanly and avoid conflicts with sidecar updates.

## [0.1.14] - 2026-01-31

### Improved

- **Ralph Loop Reliability**: Updated the prompt engineering for both Ralph Loop and Plan Execution modes to explicitly enforce the use of the `todowrite` tool. This ensures that tasks are visually marked as "completed" in the UI as the AI finishes them, preventing the state desync where work was done but tasks remained unchecked.
- **Task Execution Flow**: When executing approved tasks from the Plan sidebar, the system now provides stronger directives to the AI to update task status immediately upon completion.

## [0.1.13] - 2026-01-30

### Added

- **Ralph Loop**: Implemented iterative task execution mode with the following features:
  - New `ralph` Rust module with `RalphLoopManager`, `RalphStorage`, and `RalphRunHandle`
  - Toggle button in chat control bar to enable/disable loop mode
  - Status chip showing current iteration and status (Running/Paused/Completed/Error)
  - Side panel with pause/resume/cancel controls and context injection
  - Completion detection via `<promise>COMPLETE</promise>` token matching
  - Struggle detection after 3 iterations with no file changes or repeated errors
  - Git-based file change tracking between iterations
  - Workspace-local storage at `.opencode/tandem/ralph/` (state.json, history.json, context.md)
  - Seven Tauri commands: `ralph_start`, `ralph_cancel`, `ralph_pause`, `ralph_resume`, `ralph_add_context`, `ralph_status`, `ralph_history`
  - Plan Mode integration - Ralph respects staging and never auto-executes
  - Frontend components: `LoopToggle`, `LoopStatusChip`, `RalphPanel`
- **Memory Context System**: Integrated a semantic memory store using `sqlite-vec`. This allows the AI to store and retrieve context from past sessions and project documentation, enabling long-term memory and smarter context-aware responses.

### Fixed

- **Memory Store Initialization**: Resolved an `unresolved import sqlite_vec::sqlite_vec` error by correctly implementing the `sqlite3_vec_init` C-extension registration via `rusqlite`.

## [0.1.13] - 2026-01-30

### Added

- **Planning Mode**: Introduced a dedicated planning agent that generates comprehensive markdown-based implementation plans before executing code changes. Includes support for real-time plan file synchronization and a specialized UI for plan management.
- **Plan File Watcher**: Backend file watcher for `.opencode/plans/` that automatically updates the UI when plans are modified, ensuring the frontend is always in sync with the AI's latest proposals.
- **Ask Follow-up Question**: Integrated support for the `ask_followup_question` tool in the planning process, allowing the AI to clarify scope and technical preferences with interactive suggestion buttons.

### Fixed

- **Backend Compilation**: Resolved a critical "no method named `get_workspace_path` found" error in `commands.rs` by adding the missing method to `AppState`.
- **Tool Parsing Accuracy**: Improved sidecar communication by strictly enforcing tool name formatting (removing potential leading spaces) and correcting invalid tool examples in the plan skill instructions.

### Changed

- **Planning Flow**: Streamlined the transition from plan to execution. The AI is now instructed to generate plans immediately without conversational filler, using strict system directives.

## [0.1.12] - 2026-01-22

### Fixed

- **File Viewer**: Fixed "Failed to load directory" error by removing overly restrictive path allowlist checks that were causing Windows path normalization issues.
- **Permission Spam**: Prevented repeated approval prompts for the same tool request.
- **Allow All Auto-Approval**: Aligned auto-approval with permission request IDs to stop duplicate prompts.
- **Session Switching**: Cleared pending permission state when switching sessions to avoid stale approvals.

## [0.1.11] - 2026-01-22

### Fixed

- **Version Metadata**: Fixed version numbers in `tauri.conf.json`, `package.json`, and `Cargo.toml` to ensure proper auto-update detection. Previous release (v0.1.10) had mismatched version metadata (some files were 0.1.8 or 0.1.9 while the built version was 0.1.10), causing update failures.
- **File Access Guardrails**: Enforced workspace allowlist checks for file browsing, text reads, and binary reads to prevent unintended access outside the active workspace.
- **Windows Path Denylist**: Normalized Windows path separators so deny patterns like `.env` and key files reliably block access.
- **Binary Read Limits**: Added size limits for binary reads to avoid large base64 payloads.
- **Log Noise**: Removed verbose streaming and provider debug logs to reduce UI overhead during active sessions.

## [0.1.10] - 2026-01-22

### Added

- **Skills Management UI**: Added a complete skills management interface in Settings, allowing users to import, view, and manage OpenCode-compatible skills (both project-specific and global).
- **Skill Discovery**: Implemented automatic discovery of installed skills from both project (`.opencode/skill/`) and global (`~/.config/opencode/skills/`) directories.
- **Smart Project Selection**: Skills panel now displays the active project name and automatically disables project-specific installation when no project is selected.
- **Skill Resource Links**: Added clickable links to popular skill repositories (open skills library, SkillHub, GitHub) using Tauri's native URL opener.
- **Automatic Sidecar Restart**: Implemented seamless AI engine restart after skill import with a full-screen overlay matching the app's aesthetic. Features animated rotating icon, pulsing progress bars, and backdrop blur.

### Fixed

- **Skills Import Reliability**: Fixed critical bug where SKILL.md files with YAML frontmatter containing colons (e.g., "for: (1)") would fail to parse. The parser now automatically quotes descriptions with special characters.
- **Skills Save Format**: Fixed issue where imported skills were being reconstructed incorrectly, causing frontmatter corruption. Skills are now saved with their original content preserved.
- **TypeScript Errors**: Resolved missing `projectPath` prop type in SkillsPanel component.
- **External Links**: Fixed broken external links in Skills panel to use Tauri's `openUrl()` instead of non-functional `href` attributes.

### Changed

- **Button Styling**: Cleaned up Save button appearance by removing emoji for a more professional look.
- **Project Name Display**: Improved visual hierarchy in project selection with bold primary-colored project names and muted path indicators.
- **Error Handling**: Added comprehensive debug logging for skill discovery and YAML parsing to improve troubleshooting.
- **Auto-Refresh**: Skills list now properly refreshes after importing new skills by awaiting the refresh callback.

## [0.1.9] - 2026-01-21

### Fixed

- **macOS Styling:** Refined the glass effect styling and other UI polish to improve the overall look and feel on macOS.
- **BaseHref Support:** Added support for `baseHref` in HTML previews to correctly resolve relative paths for images and stylesheets.

## [0.1.7] - 2026-01-21

### Fixed

- **Slides Workflow Feedback Loop:** Refined the presentation guidance to be more flexible, ensuring the AI acknowledges user feedback/improvements during the planning phase instead of jumping immediately to execution.
- **"Add to Chat" Reliability:** Fixed a state management bug in `ChatInput` that prevented HTML files and other external attachments from being correctly added to the chat context.
- **Blur Obstruction:** Removed the `blur(6px)` transition from the `Message` component and streaming indicator, preventing the chat from becoming unreadable during active AI generation.
- **High-Fidelity PDF Export:** Added `@page { margin: 0; size: landscape; }` and `color-adjust` CSS to the HTML slide template to suppress browser headers/footers and preserve professional aesthetics during PDF export.
- **File Link Detection (Chat UI):** Refined the file path detection regex to only match explicit paths (containing slashes or drive letters), preventing normal text from being incorrectly rendered as "jarbled" clickable links.
- **Dynamic Ollama Discovery:** Implemented automatic model discovery for local Ollama instances. The application now dynamically generates the sidecar configuration based on actually installed local models, ensuring a seamless zero-config experience across all platforms.
- **Cross-Platform Config Reliability:** Updated the sidecar manager to correctly handle OpenCode configuration paths on Linux, macOS, and Windows, and bundled a default template in the installer for improved auto-update reliability.
- **Settings Synchronization:** Fixed a bug where changing the model/provider in settings was not immediately reflected in the Chat interface.
- **Model Selector Refinement:** Cleaned up the model dropdown to prioritize OpenCode Zen/Ollama and hide unconfigured providers, reducing clutter.
- **"Allow All" Logic:** Fixed a critical issue where the "Allow All" toggle was ignored by the event handler, implementing robust auto-approval logic for permissions.
- **Chat History Visibility:** Improved session list filtering to strictly handle project path normalization, ensuring only relevant project chats are shown while preventing history loss.

## [0.1.6] - 2026-01-20

### Added

- **High-Fidelity HTML Slides:** Replaced legacy PPTX generation with an interactive 16:9 HTML slideshow system featuring Chart.js integration, keyboard navigation, optimized PDF export via a dedicated Print button, content overflow protection, and strict density limits (max 6 items per slide).
- **Collapsible Tool Outputs:** Large tool outputs (like `todowrite` or file operations) are now collapsed by default in the chat view, reducing visual noise. Users can expand them to see full details.
- **Chart Generation Capabilities:** Updated internal marketing documentation to highlight the new capability of generating interactive visual dashboards directly from research data.
- HTML Canvas/Report feature: render interactive HTML files in a sandboxed iframe with Tailwind, Chart.js, and Font Awesome support.
- "Research" tool category with dedicated instructions for a robust "Search → Select → Fetch" workflow.
- Visibility of AI reasoning/thinking parts in both live streaming and chat history.
- Automatic persistence of the current active session across reloads/refreshes.
- Default "allow" rules for safe tools (`ls`, `read`, `todowrite`, `websearch`, `webfetch`) to reduce permission prompts.

### Fixed

- **[REDACTED] Filtering:** Removed spurious `[REDACTED]` markers that were leaking from OpenCode's internal reasoning output into the chat UI.
- **File Link Detection (Critical Fix):** Completely rewrote the file path regex to reliably detect Unix absolute paths like `/home/user/file.html` in chat messages, making them clickable.
- **Slide Layout & Scaling:** Fixed vertical stacking of slides in the HTML generator and added auto-scaling to fit the viewer's viewport dimensions.
- **Chat Error Handling:** Implemented deduplication for session error messages to prevent repeated bubbles during stream failures.
- **Linux UI Transparency:** Fixed an issue where the project switcher dropdown was unreadable on Linux due to incorrect glass effect rendering.
- **Session Loading:** Resolved a bug where the application would start with a blank screen instead of loading the previously selected chat session.
- **External Link Handling:** Fixed permission issues preventing "Open in Browser" from working for generated files.
- **HTML Preview:** Links within generated HTML reports now correctly open in the system default browser.
- **Tool Selector Cleanup:** Temporarily disabled the unimplemented "Diagrams" and "Tables" categories from the specialized tools selector to improve UX.
- Robust cancellation: Stop button now reliably terminates backend AI processes using a fallback API mechanism.
- Tool visibility: All tool calls (including technical ones) are now visible throughout the session per user request.
- Fixed chat "freezing" by ensuring intermediate reasoning and tool steps are always streamed to the UI.
- Replaced hardcoded version numbers with dynamic values in `MatrixLoader`, `Settings`, and the initial splash screen.
- Improved error handling in the sidecar manager when primary cancellation endpoints are unavailable.
- Resolved ESLint warnings in `Message.tsx` and `Chat.tsx`.

### Changed

- Updated `create_session` and `rewind_to_message` to include default safe-tool permissions.
- Modified `sidecar.rs` to treat "reasoning" parts as visible content.

## [0.1.5] - 2026-01-20

### Added

- Compact theme selector dropdown with theme details and color swatches.
- Active provider/model badge next to the tool selector in chat.
- Allow-all toggle for tool permissions on new chats.

### Fixed

- Linux `.deb` auto-update now downloads the `.deb` artifact (instead of the AppImage), preventing `update is not a valid deb package`.
- Taskbar no longer overlays the About and Settings screens.
- OpenCode sidecar API compatibility after upstream route changes (provider/model listing and prompt submission).
- Streaming event parsing for newer OpenCode SSE payload shapes.
- Structured provider errors now surface in the chat UI instead of failing silently; improved extraction of specific reasons (e.g., credit limits) from nested responses.
- Permission prompts now render correctly for updated tool event payloads.
- Provider key status refreshes immediately after saving or deleting API keys.
- Technical tool calls (edit, write, ls, etc.) are now handled as transient background tasks and auto-cleanup from chat on success.
- Final AI responses now render reliably at the end of a session, with an automatic backfill mechanism if the stream is interrupted.
- Reduced terminal log spam by downgrading verbose background activity and summarizing large event payloads.
- Fixed a TypeScript error where the `tool` property was missing from the `tool_end` event payload.

### Changed

- Update checking and install progress is now shown at the top of Settings.

## [0.1.4] - 2026-01-20

### Added

- Auto-update functionality with GitHub releases
- Sidecar binary management and updates
- Vault encryption for API keys
- About page with update checker

### Changed

- Improved sidecar process management on Windows
- Enhanced error handling for file operations

### Fixed

- File locking issues during sidecar updates on Windows
- ESLint warnings in React components

## [0.1.0] - 2026-01-18

### Added

- Initial release
- Chat interface with OpenCode AI engine
- Session management and history
- Multi-provider support (Anthropic, OpenAI, OpenRouter)
- Zero-trust security with local encryption
- Project-based organization
- Real-time streaming responses

[0.3.17]: https://github.com/frumu-ai/tandem/compare/v0.3.16...HEAD
[0.3.16]: https://github.com/frumu-ai/tandem/compare/v0.3.15...v0.3.16
[0.3.15]: https://github.com/frumu-ai/tandem/compare/v0.3.14...v0.3.15
[0.3.14]: https://github.com/frumu-ai/tandem/compare/v0.3.13...v0.3.14
[0.3.12]: https://github.com/frumu-ai/tandem/compare/v0.3.11...v0.3.12
[0.3.11]: https://github.com/frumu-ai/tandem/compare/v0.3.10...v0.3.11
[0.3.10]: https://github.com/frumu-ai/tandem/compare/v0.3.9...v0.3.10
[0.3.9]: https://github.com/frumu-ai/tandem/compare/v0.3.7...v0.3.9
[0.3.7]: https://github.com/frumu-ai/tandem/compare/v0.3.6...v0.3.7
[0.3.6]: https://github.com/frumu-ai/tandem/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/frumu-ai/tandem/compare/v0.3.2...v0.3.5
[0.3.2]: https://github.com/frumu-ai/tandem/compare/v0.3.1...v0.3.2
[0.2.25]: https://github.com/frumu-ai/tandem/compare/v0.2.24...v0.2.25
[0.2.24]: https://github.com/frumu-ai/tandem/compare/v0.2.23...v0.2.24
[0.2.23]: https://github.com/frumu-ai/tandem/compare/v0.2.22...v0.2.23
[0.2.22]: https://github.com/frumu-ai/tandem/compare/v0.2.21...v0.2.22
[0.2.21]: https://github.com/frumu-ai/tandem/compare/v0.2.20...v0.2.21
[0.2.19]: https://github.com/frumu-ai/tandem/compare/v0.2.18...v0.2.19
[0.2.18]: https://github.com/frumu-ai/tandem/compare/v0.2.17...v0.2.18
[0.2.17]: https://github.com/frumu-ai/tandem/compare/v0.2.16...v0.2.17
[0.2.16]: https://github.com/frumu-ai/tandem/compare/v0.2.15...v0.2.16
[0.2.15]: https://github.com/frumu-ai/tandem/compare/v0.2.14...v0.2.15
[0.2.14]: https://github.com/frumu-ai/tandem/compare/v0.2.13...v0.2.14
[0.2.13]: https://github.com/frumu-ai/tandem/compare/v0.2.12...v0.2.13
[0.2.12]: https://github.com/frumu-ai/tandem/compare/v0.2.11...v0.2.12
[0.2.11]: https://github.com/frumu-ai/tandem/compare/v0.2.10...v0.2.11
[0.2.10]: https://github.com/frumu-ai/tandem/compare/v0.2.9...v0.2.10
[0.2.9]: https://github.com/frumu-ai/tandem/compare/v0.2.8...v0.2.9
[0.2.8]: https://github.com/frumu-ai/tandem/compare/v0.2.7...v0.2.8
[0.1.13]: https://github.com/frumu-ai/tandem/compare/v0.1.12...v0.1.13
[0.1.12]: https://github.com/frumu-ai/tandem/compare/v0.1.11...v0.1.12
[0.1.11]: https://github.com/frumu-ai/tandem/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/frumu-ai/tandem/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/frumu-ai/tandem/compare/v0.1.7...v0.1.9
[0.1.8]: https://github.com/frumu-ai/tandem/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/frumu-ai/tandem/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/frumu-ai/tandem/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/frumu-ai/tandem/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/frumu-ai/tandem/compare/v0.1.0...v0.1.4
[0.1.0]: https://github.com/frumu-ai/tandem/releases/tag/v0.1.0
