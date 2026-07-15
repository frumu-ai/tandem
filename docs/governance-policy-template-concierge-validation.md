# Governance policy template concierge validation

Status: protocol approved for use; no validation sessions recorded yet.

The CRM, finance, and coding policy templates are draft, experimental starting points. They are not validated recommendations. This protocol must be followed before any template is promoted to stable.

## Session requirements

Run at least five distinct, observed sessions across the intended buyer profiles. Include at least one CRM workflow, one finance workflow, one coding workflow, and two sessions selected from the strongest active design-partner demand. A session counts only when a real operator attempts to adapt a template to a real workflow and explains their choices; internal walkthroughs and synthetic examples do not count.

Before each session, record the participant profile, domain, workflow, expected tools, sensitive data classes, approval roles, and deployment mode. During the session, preserve the operator's corrections, policy misunderstandings, requested overrides, and any point where the non-overridable deny floor blocks or surprises them. Do not record raw credentials or customer payloads.

## Evidence record

Create one evidence record per session with:

- session ID, date, facilitator, participant profile, and consented evidence link;
- template ID and version;
- deployment mode and connector/tool scope;
- operator's intended allow, deny, and approval boundaries;
- observed misunderstandings and facilitator interventions;
- requested overrides and whether the current override model supports them;
- deny-floor behavior and any attempted weakening;
- resulting template changes, or an explicit no-change decision;
- participant confidence before and after configuration.

## Success criteria

A template is eligible for a human go/no-go review only when all of the following are true:

1. Five distinct qualifying sessions are linked.
2. At least four participants can correctly predict the outcome of the template's representative allow, deny, and approval examples without facilitator correction.
3. Every requested override is either supported safely or documented as an intentional denial.
4. No session reveals a path that weakens a non-overridable credential, secret, or protected-branch deny floor.
5. Changes are versioned and the changelog links the session evidence that motivated each change.
6. Upgrade and rollback tests pass for the proposed version.

The final promotion requires an explicit human `go` decision, approver identity, timestamp, and the five session IDs. The API contract rejects promotion without that evidence. Until then, the Control Panel must display the templates as `Draft · experimental`.

## Session ledger

| Session | Domain/profile | Template/version | Evidence | Result |
| --- | --- | --- | --- | --- |
| Pending 1 | — | — | — | Not run |
| Pending 2 | — | — | — | Not run |
| Pending 3 | — | — | — | Not run |
| Pending 4 | — | — | — | Not run |
| Pending 5 | — | — | — | Not run |

Do not replace `Not run` with a result until the corresponding evidence record exists.
