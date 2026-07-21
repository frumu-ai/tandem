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
| Pack reads/detect/install/uninstall/export | action-specific capability | Mutation requirements are classified in code; pack migration remains pending |

## Resource rules

Hosted file and repository operations accept opaque session/repository IDs.
The server loads the session, enforces the same actor, resolves the configured
workspace, canonicalizes it, and refuses a Git top-level that would widen the
resource to an ancestor.

Managed worktree mutations additionally require:

- a stored managed record;
- exact repository, tenant, deployment, and actor ownership;
- a request lease equal to the record lease;
- an active lease owned by that tenant;
- a path below .tandem/worktrees;
- an exact opaque worktree ID for verified callers; and
- deployment-admin authority for delete/reset.

Hosted responses return opaque resource IDs and omit canonical host paths and
raw process errors.

## Migration rule

Adding an authorization check only to a route is insufficient. State managers,
background jobs, maintenance cleanup, retry paths, and other direct callers must
receive and revalidate a grant or obtain an audited InternalRuntime grant from
stored canonical resource state.

Unmigrated host-effect surfaces must remain disabled or edge-blocked under the
containment advisory until their dedicated remediation and focused retest are
complete.
