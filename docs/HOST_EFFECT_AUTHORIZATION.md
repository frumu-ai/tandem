# Host-effect authorization contract

Host effects are operations that cross from request or workflow state into the
engine host: file reads/searches, process creation, browser installation or
navigation, storage repair, global disposal, managed Git worktrees, and pack
lifecycle operations.

Every migrated effect uses action_authorization and follows the same order:

1. resolve the canonical resource and its tenant/actor from server-side state;
2. build a HostEffectRequest containing the explicit action, canonical
   resource, and exact arguments;
3. authorize an explicit capability or the deployment-admin requirement;
4. durably append the protected-audit grant;
5. revalidate the short-lived grant against the unchanged request immediately
   before the host effect; and
6. execute with server-owned limits and return redacted hosted output.

The grant fields are private. Its SHA-256 effect digest binds the serialized
action, resource kind/ID/owner context, and exact argument object. Changing a
path, executable, argument, target ref, lease, limit, or recovery ref invalidates
the grant.

## Authority sources

VerifiedCapability requires a non-expired verified context whose
org/workspace/deployment/actor and human actor exactly match the request
context. Roles do not imply capabilities. deployment.admin may satisfy
non-admin host actions, but an action classified as deployment-admin-only
cannot be authorized by its action-specific capability alone.

LoopbackLocalOwner is a compatibility posture for a standalone engine only.
It requires all of:

- an accepted peer address that is loopback;
- no forwarding/proxy headers;
- a loopback listener and loopback configured base URL;
- the implicit local tenant; and
- no hosted/enterprise verified context.

The condition is checked again during grant revalidation.

InternalRuntime is for trusted in-process workers that act on a resource
resolved from stored state. It does not skip the exact request digest, short
expiry, protected-audit commit, or final-boundary revalidation. It exists so a
background cleanup/worker cannot bypass a check that appears only in an HTTP
handler.

## Capability map

| Effect | Capability | Deployment admin required |
| --- | --- | --- |
| File read/search | host.files.read | No |
| Fixed structured command | host.command.execute | No |
| PTY management | host.pty.manage | No; direct HTTP routes are currently removed |
| Worktree list/create | host.worktree.read, host.worktree.create | No |
| Worktree delete/reset/cleanup | action-specific capability | Yes |
| Browser install/smoke test | action-specific capability | Yes |
| Storage repair/global dispose | action-specific capability | Yes |
| Pack reads/detect/install/uninstall/export | action-specific capability | Hosted host-path operations are disabled; full managed-resource migration remains pending |

## Resource rules

Hosted file and repository operations accept opaque session/repository IDs.
The server loads the session, enforces the same actor, resolves the configured
workspace, canonicalizes it, and refuses a Git top-level that would widen the
resource to an ancestor.

File-content reads traverse the canonical workspace using descriptor-relative
no-follow opens on Unix, so a symlink swap after authorization cannot redirect
the opened file. Verified file-content reads fail closed on platforms without
that primitive until an equivalent handle-based implementation is available.

Managed worktree mutations additionally require:

- a stored managed record;
- exact repository, tenant, deployment, and actor ownership;
- a request lease equal to the record lease;
- an active lease owned by that tenant;
- a path below .tandem/worktrees;
- an exact opaque worktree ID for verified callers; and
- deployment-admin authority for delete/reset.

Managed worktree parents and targets are checked component-by-component with
symlink metadata and canonical equality immediately before each effect. All Git
commands in managed-worktree paths run with a cleared allowlisted environment,
external helpers disabled, repository content-filter drivers overridden to no-op for
checkout-producing commands, a 15-second deadline, and capped stdout/stderr.
Removal never uses `--force`; Git performs its own final dirty-state refusal so
a concurrent modification is preserved instead of erased. Reset repeats the
dirty check at the final boundary and uses Git's local-change-preserving mode.
Orphan directories are recursively removed relative to retained no-follow
directory handles on Unix; verified cleanup fails closed without that primitive.

Hosted responses return opaque resource IDs and omit canonical host paths and
raw process errors.

The compatibility `/worktree/cleanup` route is available to the standalone
loopback owner. In a verified deployment it resolves an opaque tenant-owned
repository resource, requires deployment-admin authority, binds the cleanup
options into the exact effect digest, revalidates before every removal, and
returns counts without host paths or raw Git errors.

Pack host-path operations fail closed outside the standalone loopback-local
posture until their inputs are migrated to managed resource IDs and grants.

## Migration rule

Adding an authorization check only to a route is insufficient. State managers,
background jobs, maintenance cleanup, retry paths, and other direct callers must
receive and revalidate a grant or obtain an audited InternalRuntime grant from
stored canonical resource state.

Unmigrated host-effect surfaces must remain disabled or edge-blocked under the
containment advisory until their dedicated remediation and focused retest are
complete.
