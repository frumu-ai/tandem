# Approval Gates by Default + Rich Channel UX

This folder is the committed working plan for making human-approval gates a default behavior in Tandem workflows and surfacing them as native interactive cards in Slack, Discord, and Telegram.

## Files

- [`PLAN.md`](./PLAN.md) — full implementation plan: 5-week sequencing, critical files with line numbers, reuse map, risks, channel UX recommendations, verification steps.

## Why this exists

The product pitch is governed AI operations. Today the runtime has approval primitives (`HumanApprovalGate`, `AutomationPendingGate`, gate-decide handlers) but **workflows never pause** — `crates/tandem-server/src/workflows.rs:1209` hard-codes `approval_state: Some("executed")`. No shipped marketplace pack uses gates. Until this is fixed, "agents under runtime governance" is a slogan.

This work closes the gap end-to-end: server-side pause/resume, action classifier, compiler injection pass, control-panel inbox, ScopeInspector override UI, rich interactive cards in all three channels with proper signing and idempotency.

## How this connects to other plans

- **Reality map** — [`../internal/business/replit-for-operations-reality-map.md`](../internal/business/replit-for-operations-reality-map.md) §7 lists "approval gates wired into the demo workflow" as a gap blocking the first paid pilot. This plan fills that gap.
- **Executive summary** — [`../internal/business/replit-for-operations-executive-summary.md`](../internal/business/replit-for-operations-executive-summary.md) lists the IP-leak risk + the deployment-control story; the approvals work is what makes the "Tandem-operated managed pilot" actually governable.
- **Demo plan** — [`../internal/demo/demo-analysis-plan.md`](../internal/demo/demo-analysis-plan.md) §0 (Reality Check) and §8 (Tier 0 build list, items tagged `[GAP]`) call out the approval gap as a demo blocker. This plan unblocks Tier 0.
- **Demo readiness checklist** — [`../internal/demo/demo-readiness-checklist.md`](../internal/demo/demo-readiness-checklist.md) Tier 0 includes "approval gates wired into the demo workflow" as a hard go/no-go before the recorded video can ship.

## Scope discipline

The plan deliberately ships a Slack vertical slice end-to-end in week 2 (before going wide) so auth, race, and idempotency surface while the data model is still soft. Discord and Telegram inherit the patterns in week 4. Out-of-scope items (email approvals, reactions-as-approve, cross-tenant delegation, mobile push, SSE on the inbox) are listed explicitly in `PLAN.md` and should not be re-scoped without an updated plan.

## Status

- Plan: drafted and approved 2026-04-26.
- Implementation: not started. Awaiting engineer assignment and timeline confirmation.
