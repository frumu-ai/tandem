# Security containment advisory (2026 audit)

This advisory applies while the security-audit remediation project is open. All
confirmed finding implementations have merged, but the cross-cutting inventory,
dynamic retest, assurance scan, and final release decision remain open. This is
a temporary deployment boundary, not a final release decision.

## Deployment scope recorded for this audit

No enterprise or shared Tandem server has been deployed. The audit therefore
does not identify a running customer, enterprise, or shared-engine instance that
was exposed by these findings, and it does not assert that a standalone local
engine was remotely exposed.

Credential rotation is **not required solely because a source finding existed**
when there was no deployed/shared runtime and no real credential was placed in
one. Rotation remains mandatory if deployment records, operator evidence, or a
later incident review shows that a runtime was reachable/shared or held a real
credential outside the supported standalone posture. This scope decision must
be re-evaluated before the first hosted, shared, or enterprise deployment.

## Supported interim posture

Run Tandem as a single-user standalone engine bound directly to a loopback
address. Keep generated transport authentication enabled. Do not expose the
engine port through a public listener, shared reverse proxy, tunnel, container
port publication, or multi-tenant service.

The following postures remain blocked until the focused security retest records
a release decision:

- remotely reachable engine API;
- shared or multi-tenant engine process;
- hosted or enterprise deployment using this standalone engine as a shared
  execution service;
- any configuration that disables the transport token; and
- any Web UI prefix outside the reserved /admin or /ui namespaces.

TANDEM_UNSAFE_NO_API_TOKEN must remain unset or false.
TANDEM_WEB_UI_PREFIX should remain /admin. The server normalizes
API-overlapping, encoded-separator, traversal, and non-reserved prefixes back to
/admin.

## Edge containment

Until the final focused retest records an allow decision, an upstream edge must
deny these route families if the engine is reachable beyond its standalone
loopback owner:

- pack install, update, uninstall, export, detection, and file access;
- MCP definition and connection mutation;
- provider and global configuration mutation, including token rotation;
- channel configuration, credential, reload, and destination mutation;
- channel enrollment, step-up grants, and sender-inventory reads;
- permission/question decisions and workflow/governance approval mutation; and
- browser-sidecar download or installation.

Pack install, attachment install, uninstall, export, and detect handlers also
fail closed in verified/non-local postures. The compatibility worktree cleanup
route is restricted to the standalone loopback owner or a verified
deployment-admin grant over an opaque tenant-owned repository.

Direct PTY routes are intentionally absent. The legacy /session/{id}/shell alias returns 403. The remaining direct command
endpoint accepts only fixed, non-interpreter Git inspection presets and applies
an exact capability/resource/effect grant before process creation.

File reads and searches are bounded and resolve a server-owned session resource.
Destructive managed-worktree delete/reset operations require a stored managed
record, matching tenant and actor, an active exact lease, and deployment-admin
authority outside the explicit standalone loopback-owner posture.

Managed-worktree paths with symlinked parents or targets are rejected. Git runs
with a cleared environment, disabled hooks/fsmonitor/content filters, and strict
time/output bounds. Removal is force-free, reset repeats its dirty check and
uses local-change-preserving mode, and orphan deletion is descriptor-relative on
Unix. File-content reads use descriptor-relative no-follow traversal on Unix to
close symlink-swap races at open time.

## Credential response

Do not place production provider, channel, signing, or deployment credentials
in a shared runtime while this advisory is active.

For any runtime that was previously reachable/shared, or that held real shared
credentials:

1. remove it from service and preserve logs needed for incident review;
2. inventory transport, provider, channel, webhook, OAuth, signing, and sidecar
   credentials available to that runtime;
3. review configuration/destination change history and protected-audit records;
4. rotate credentials at the authoritative provider, not only in Tandem;
5. verify old credentials are rejected; and
6. constrain outbound destinations to the approved service origins before
   restoring even a loopback-only instance.

Never paste rotated secret values into issues, pull requests, logs, or audit
documents.

## Smoke test

Before starting the engine:

1. confirm the configured listener is loopback;
2. confirm a transport token is present and unsafe-token mode is disabled;
3. confirm the Web UI prefix is /admin;
4. confirm the edge deny rules above are present if any proxy exists; and
5. confirm unsigned browser/engine artifacts cannot be selected or executed.

After starting:

1. verify GET /global/health returns only liveness/readiness fields;
2. probe the engine port from a non-loopback host and confirm it is unreachable;
3. verify /pty and /pty/{id} return 404;
4. verify /session/{id}/shell returns 403; and
5. verify an ordinary authenticated principal cannot invoke `/worktree/cleanup`
   or any hosted pack host-path operation; and
6. verify an ordinary authenticated principal cannot invoke browser install,
   browser smoke testing, storage repair, global disposal, or destructive
   worktree mutation; and
7. verify channel enrollment, channel step-up grants, and Slack sender inventory
   reject an ordinary tenant principal, reject cross-tenant targets, and allow
   only an exact deployment-admin grant or the direct standalone loopback owner.

## Rollback

Rollback means returning to a loopback-only standalone engine or stopping the
engine. It does not mean re-enabling a vulnerable route to restore a hosted
workflow. If a remediation causes an operational regression, stop the affected
workflow, preserve the failing request ID and protected-audit evidence, and
revert only within the loopback-only standalone posture.
