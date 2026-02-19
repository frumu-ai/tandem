# Agent Command Center Kanban

Updated: 2026-02-19
Owner: Platform / Agent Runtime

## In Progress

- [ ] `ACC-007` Replace polling refresh with SSE-driven live updates in desktop command center.
- [ ] `ACC-008` Normalize tool-approval payload shape from server so UI can always approve/deny without fallback parsing.

## Ready Next

- [ ] `ACC-009` Add mission timeline/event rail (spawn chain, state transitions, failures, cancellations).
- [ ] `ACC-010` Add mission/instance search + filter chips (role, status, mission, parent).
- [ ] `ACC-011` Add guided spawn flow for non-developers (simple mode + advanced mode).
- [ ] `ACC-012` Add command-center “health strip” (SSE connected, last event time, refresh mode).

## Backlog

- [ ] `ACC-013` Add desktop approvals inbox combining spawn approvals and tool approvals in one queue.
- [ ] `ACC-014` Add TUI command-center parity (`/agent-team` dashboard + approval actions).
- [ ] `ACC-015` Add role/template editor UX in desktop with safe policy validation preview.
- [ ] `ACC-016` Add exportable mission run report (JSON + markdown summary).
- [ ] `ACC-017` Add operator onboarding tour for first-time command-center users.

## Done

- [x] `ACC-001` Add server-side spawn approval decision endpoints:
  - `POST /agent-team/approvals/spawn/{id}/approve`
  - `POST /agent-team/approvals/spawn/{id}/deny`
- [x] `ACC-002` Add Tauri sidecar agent-team bridge methods for templates/missions/instances/approvals/spawn/cancel/decide.
- [x] `ACC-003` Add Tauri IPC commands exposing command-center actions to desktop frontend.
- [x] `ACC-004` Add desktop Agent Command Center surface in orchestrator panel.
- [x] `ACC-005` Add command-center spawn approvals actions (approve/deny) in desktop UI.
- [x] `ACC-006` Add mission and instance drill-down details + tool-approval action path in desktop UI.

## Risks / Notes

- Tool approval payload currently arrives as loosely typed JSON in `/agent-team/approvals`; parser fallback is in place, but a strict typed contract is still pending (`ACC-008`).
- Current desktop live view uses polling; SSE integration is needed for smoother updates and lower latency (`ACC-007`).
