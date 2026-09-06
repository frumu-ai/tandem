# Native solution agent staging

`AgentTeamRuntime::stage_solution_template` writes an actual disabled
`AgentTemplate` into the existing workspace template directory. It forces
`enabled: false` and binds organization, workspace, deployment, installation,
component and reviewed composition in `solution_owner`. It returns a canonical
fingerprint of the observed resource for the existing installation journal.

The caller must authorize the selected installation, reload its current journal,
verify the current signed artifact and host-owned bindings, and claim the
component before this effect. Metadata and a composition hash are not grants.
This native adapter does not yet supply that HTTP/service orchestration.

Publication writes and syncs a complete temporary file, then uses an atomic
non-replacing hard link in the same directory. A crash leaves an ignored
temporary file or a complete disabled template. Retry compares the exact typed
content and ownership on disk before reporting the same receipt. Conflicting
manual content or another installation is never overwritten. Filesystems that
do not support hard links fail closed. Unix also syncs the containing directory;
Windows power-loss durability of the directory entry is not established by the
process-restart tests.

Old template documents default to enabled. Disabled templates cannot pass spawn
policy, including the approval override path, and are excluded from automatic
role selection. Generic template upsert/delete cannot mutate managed templates
or the reserved `solution-` ID namespace. Existing manually created IDs in that
namespace therefore require an operator-reviewed migration before editing;
the adapter does not take them over. Installed IDs must also be portable under
the existing template filename mapping (ASCII letters, digits, hyphen, underscore).

Only reusable pack content belongs in template files. Customer records, private
notes and credentials stay in their governed stores. This does not implement
routine staging, live host binding resolution, installation authorization,
activation, upgrade, rollback, shared resource deletion, UI/CLI or clean-host
recovery. Staged resources are not a ready Company Brain product.
