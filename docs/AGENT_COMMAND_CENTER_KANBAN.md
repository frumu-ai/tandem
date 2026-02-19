# Agent Command Center Kanban

Updated: 2026-02-19
Owner: Platform / Agent Runtime

## In Progress

- [ ] `ACC-015` Add role/template editor UX in desktop with safe policy validation preview.
- [ ] Harden command-center payload guards to tolerate mixed snake_case/camelCase responses.

## Ready Next

- [ ] `ACC-017` Add operator onboarding tour for first-time command-center users.

## Backlog

- [ ] Add command-center smoke tests (desktop integration path).

## Done

- [x] `ACC-001` Add server-side spawn approval decision endpoints:
  - `POST /agent-team/approvals/spawn/{id}/approve`
  - `POST /agent-team/approvals/spawn/{id}/deny`
- [x] `ACC-002` Add Tauri sidecar agent-team bridge methods for templates/missions/instances/approvals/spawn/cancel/decide.
- [x] `ACC-003` Add Tauri IPC commands exposing command-center actions to desktop frontend.
- [x] `ACC-004` Add desktop Agent Command Center surface in orchestrator panel.
- [x] `ACC-005` Add command-center spawn approvals actions (approve/deny) in desktop UI.
- [x] `ACC-006` Add mission and instance drill-down details + tool-approval action path in desktop UI.
- [x] `ACC-007` Add SSE-driven refresh trigger in desktop command center for `agent_team.*` event stream updates (polling fallback retained).
- [x] `ACC-008` Normalize `/agent-team/approvals` tool-approval payload contract (`approvalID`, `sessionID`, `toolCallID`, `tool`, `args`, `status`) and consume typed shape in desktop.
- [x] `ACC-009` Add mission timeline/event rail in desktop command center (spawn chain + status/failure/cancel activity).
- [x] `ACC-010` Add mission/instance search + filter chips (role, status, mission, parent).
- [x] `ACC-011` Add guided spawn flow for non-developers (simple mode + advanced mode).
- [x] `ACC-012` Add command-center health strip (SSE connected, last event time, refresh mode).
- [x] `ACC-013` Add desktop approvals inbox combining spawn approvals and tool approvals in one queue.
- [x] `ACC-014` Add TUI command-center parity (`/agent-team` dashboard + approval actions).
- [x] `ACC-016` Add exportable mission run report (JSON + markdown summary).

## Risks / Notes

- Current desktop live view keeps polling as fallback; SSE now acts as a fast-path trigger for command-center refresh.
