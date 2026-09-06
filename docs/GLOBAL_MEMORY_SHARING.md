# Personal and tenant-shared global memory

Global memory uses the existing verified tenant, private-owner and department
boundaries. `tenant_shared: true` is an explicit sharing label; it never changes
the verified tenant or supplies a data-access grant.

For new `/memory/put` records:

| Request | Stored ownership | Read behavior |
| --- | --- | --- |
| `private: true`, `tenant_shared: true`, no department | Verified owner, no department | Owner retains access after a department change, subject to current data policy |
| `private: false`, `tenant_shared: true`, no department | No private owner or department | Current authorized readers in the tenant may read across departments |
| Explicit `owner_org_unit_id`, with either sharing label | Membership-validated department; private owner if requested | Department remains mandatory; a private record additionally requires its owner |
| No sharing label or a non-boolean label | Existing automatic department stamp | Existing department/private behavior |

SQLite and PostgreSQL persist `tenant_shared` as a first-class global-record
column. Search, list, point reads and scoped mutations admit a department-free
shared record only after independently applying tenant and private-owner checks.
Ordinary and atomic writes, deduplication and context updates retain the flag.
Queries do not extract authorization from encrypted JSON payloads.

Schema upgrades default existing rows to unshared and retain their existing
owner/department columns. A legacy label cannot establish whether an existing
department was explicitly selected or automatically stamped, so migration does
not remove it. Reclassifying existing records requires an authorized update;
this change is not a bulk reclassification tool.

The shared backend regression exercises initial and changed-department reads,
anonymous/cross-user/cross-tenant exclusion, explicit-department precedence,
normal/atomic writes and updates, deduplication and reopen. SQLite also tests
the LIKE fallback without FTS; both backends test a conservative schema upgrade.
The hosted integration in tandem-agents PR64 supplies actual authenticated
membership-change/restart acceptance. Green storage tests alone do not prove
that end-to-end acceptance, encrypted key recovery or off-site restore.
