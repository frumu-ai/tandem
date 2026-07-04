import { LazyJson } from "./LazyJson";
import {
  contextPackStateHint,
  contextPackStateLabel,
  contextPackStateTone,
  contextPackVisibilityLabel,
  kv,
  safeString,
  timestampLabel,
  toArray,
} from "./scopeInspectorPrimitives";

type SharedWorkflowContextPanelProps = {
  workspaceRoot: string;
  projectKey: string;
  sharedContextBindingRows: any[];
  supersededContextBindingRows: any[];
  contextPacks: any[];
  selectedContextPack: any;
  suggestedContextPacks: any[];
  sharedContextAllowlistInput: string;
  contextPackStatus: string;
  onAllowlistChange: (value: string) => void;
  onSelectContextPack: (packId: string) => void;
  onPublishCurrentContextPack: () => void;
  onContextPackStatusChange: (status: string) => void;
  onReplaceSharedContextPack?: (fromPackId: string, toPackId: string) => void;
};

export function SharedWorkflowContextPanel({
  workspaceRoot,
  projectKey,
  sharedContextBindingRows,
  supersededContextBindingRows,
  contextPacks,
  selectedContextPack,
  suggestedContextPacks,
  sharedContextAllowlistInput,
  contextPackStatus,
  onAllowlistChange,
  onSelectContextPack,
  onPublishCurrentContextPack,
  onContextPackStatusChange,
  onReplaceSharedContextPack,
}: SharedWorkflowContextPanelProps) {
  async function copyContextPackId(packId: string) {
    try {
      await navigator.clipboard.writeText(packId);
      onContextPackStatusChange(`Copied ${packId}.`);
    } catch (error) {
      onContextPackStatusChange(error instanceof Error ? error.message : "Copy failed.");
    }
  }

  return (
    <div className="grid gap-2">
      {sharedContextBindingRows.length ? (
        <div className="grid gap-2">
          <div className="font-medium text-slate-200">Shared context bindings</div>
          {supersededContextBindingRows.length ? (
            <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-100">
              <div className="font-medium text-amber-50">Replacement available</div>
              <div className="mt-1">
                One or more bindings point at superseded packs. Rebind them to the suggested
                replacement pack before saving this workflow.
              </div>
            </div>
          ) : null}
          <div className="grid gap-2">
            {sharedContextBindingRows.map((entry: any) => (
              <div
                key={entry.packId}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">
                    {entry.alias || entry.pack?.title || entry.packId}
                  </div>
                  <div className="flex flex-wrap items-center gap-1">
                    <span className={entry.required ? "tcp-badge-warning" : "tcp-badge-info"}>
                      {entry.required ? "required" : "optional"}
                    </span>
                    {entry.pack ? (
                      <span
                        className={
                          contextPackStateTone(entry.pack.state, entry.pack.isStale) === "success"
                            ? "tcp-badge-success"
                            : "tcp-badge-warning"
                        }
                      >
                        {contextPackStateLabel(entry.pack.state, entry.pack.isStale)}
                      </span>
                    ) : (
                      <span className="tcp-badge-info">unresolved</span>
                    )}
                  </div>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  {kv("context id", entry.packId)}
                  {kv("source plan", entry.pack?.sourcePlanId || "n/a")}
                  {kv("workspace", entry.pack?.raw?.workspace_root || "n/a")}
                  {kv("project", entry.pack?.projectKey || "n/a")}
                </div>
                {contextPackStateHint(entry) ? (
                  <div className="mt-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 tcp-text-caption text-amber-100">
                    {contextPackStateHint(entry)}
                  </div>
                ) : null}
                {entry.pack?.state === "superseded" &&
                safeString(entry.pack?.raw?.superseded_by_pack_id) ? (
                  <div className="mt-2 flex flex-wrap items-center gap-2 rounded-md border border-amber-500/30 bg-amber-500/5 p-2">
                    <div className="tcp-text-caption text-amber-100">
                      Suggested replacement:{" "}
                      <span className="font-medium">
                        {safeString(entry.pack.raw.superseded_by_pack_id)}
                      </span>
                    </div>
                    {onReplaceSharedContextPack ? (
                      <button
                        type="button"
                        className="tcp-btn h-7 px-2 tcp-text-caption"
                        onClick={() =>
                          onReplaceSharedContextPack(
                            entry.packId,
                            safeString(entry.pack.raw.superseded_by_pack_id)
                          )
                        }
                      >
                        <i data-lucide="refresh-cw"></i>
                        Swap to replacement
                      </button>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ))}
          </div>
        </div>
      ) : null}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="font-medium text-slate-200">Shared workflow context</div>
        <button
          type="button"
          className="tcp-btn h-7 px-2 tcp-text-caption"
          onClick={onPublishCurrentContextPack}
          disabled={!workspaceRoot}
        >
          <i data-lucide="package-plus"></i>
          Publish shared workflow context
        </button>
      </div>
      <div className="tcp-subtle tcp-text-caption">
        workspace: {workspaceRoot || "n/a"}
        {projectKey ? ` · project: ${projectKey}` : ""}
      </div>
      <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
        <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
          cross-project allowlist
        </div>
        <input
          type="text"
          className="tcp-input mt-2"
          value={sharedContextAllowlistInput}
          onChange={(event) => onAllowlistChange(event.currentTarget.value)}
          placeholder="project-b, project-c"
        />
        <div className="tcp-subtle mt-2 tcp-text-caption">
          Optional comma-separated project keys for future cross-project reuse. Leave blank for
          same-project only.
        </div>
      </div>
      {contextPackStatus ? (
        <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 tcp-text-caption text-slate-200">
          {contextPackStatus}
        </div>
      ) : null}
      <div className="grid gap-2">
        {contextPacks.length ? (
          <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
            <div className="grid gap-2">
              {contextPacks.map((pack: any) => {
                const isSelected = selectedContextPack?.packId === pack.packId;
                return (
                  <button
                    key={pack.packId}
                    type="button"
                    className={[
                      "rounded-lg border p-3 text-left transition",
                      isSelected
                        ? "border-blue-500/80 bg-blue-500/10"
                        : "border-slate-800/80 bg-slate-950/30 hover:border-slate-700",
                    ].join(" ")}
                    onClick={() => onSelectContextPack(pack.packId)}
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="font-medium text-slate-100">{pack.title}</div>
                      <div className="flex flex-wrap items-center gap-1">
                        {pack.visibilityScope === "project_allowlist" ? (
                          <span className="tcp-badge-info">allowlist</span>
                        ) : null}
                        {pack.isStale ? <span className="tcp-badge-warning">stale</span> : null}
                        <span className="tcp-badge-info">{pack.state}</span>
                      </div>
                    </div>
                    <div className="mt-2 grid gap-2 sm:grid-cols-2">
                      {kv("context id", pack.packId)}
                      {kv("source plan", pack.sourcePlanId || "n/a")}
                      {kv("bindings", pack.bindings.length)}
                      {kv("freshness", pack.freshnessWindowHours || "n/a")}
                    </div>
                  </button>
                );
              })}
            </div>
            <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
              {selectedContextPack ? (
                <div className="grid gap-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <div className="font-medium text-slate-100">{selectedContextPack.title}</div>
                      <div className="tcp-subtle tcp-text-caption">
                        Published {timestampLabel(selectedContextPack.raw?.published_at_ms)}
                      </div>
                    </div>
                    <div className="flex flex-wrap items-center gap-1">
                      {selectedContextPack.isStale ? (
                        <span className="tcp-badge-warning">stale</span>
                      ) : null}
                      <span className="tcp-badge-info">{selectedContextPack.state}</span>
                      <button
                        type="button"
                        className="tcp-btn h-7 px-2 tcp-text-caption"
                        onClick={() => void copyContextPackId(selectedContextPack.packId)}
                      >
                        <i data-lucide="copy"></i>
                        Copy context id
                      </button>
                    </div>
                  </div>
                  <div className="grid gap-2 sm:grid-cols-2">
                    {kv("context id", selectedContextPack.packId)}
                    {kv("workspace", selectedContextPack.raw?.workspace_root || "n/a")}
                    {kv("project", selectedContextPack.projectKey || "n/a")}
                    {kv(
                      "visibility",
                      contextPackVisibilityLabel(selectedContextPack.raw?.visibility_scope)
                    )}
                    {kv("source plan", selectedContextPack.sourcePlanId || "n/a")}
                    {kv(
                      "source automation",
                      selectedContextPack.raw?.source_automation_id || "n/a"
                    )}
                    {kv("source run", selectedContextPack.raw?.source_run_id || "n/a")}
                    {kv(
                      "source context run",
                      selectedContextPack.raw?.source_context_run_id || "n/a"
                    )}
                  </div>
                  <div className="grid gap-2 sm:grid-cols-3">
                    {kv("freshness window", selectedContextPack.freshnessWindowHours || "n/a")}
                    {kv("updated", timestampLabel(selectedContextPack.updatedAtMs))}
                    {kv("superseded by", selectedContextPack.raw?.superseded_by_pack_id || "n/a")}
                  </div>
                  <div className="grid gap-2 sm:grid-cols-2">
                    {kv(
                      "allowed projects",
                      selectedContextPack.allowedProjectKeys.length
                        ? selectedContextPack.allowedProjectKeys.join(", ")
                        : "n/a"
                    )}
                    {kv(
                      "visibility scope",
                      contextPackVisibilityLabel(selectedContextPack.raw?.visibility_scope)
                    )}
                  </div>
                  {selectedContextPack.raw?.summary ? (
                    <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                      <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">summary</div>
                      <div className="mt-1 break-words text-sm text-slate-100">
                        {selectedContextPack.raw.summary}
                      </div>
                    </div>
                  ) : null}
                  <div className="grid gap-2 sm:grid-cols-3">
                    {kv(
                      "approved materialization",
                      selectedContextPack.raw?.manifest?.approved_plan_materialization
                        ? "present"
                        : "n/a"
                    )}
                    {kv(
                      "plan package",
                      selectedContextPack.raw?.manifest?.plan_package ? "present" : "n/a"
                    )}
                    {kv(
                      "runtime context",
                      selectedContextPack.raw?.manifest?.runtime_context ? "present" : "n/a"
                    )}
                  </div>
                  <div className="grid gap-2 sm:grid-cols-3">
                    {kv(
                      "context refs",
                      toArray(selectedContextPack.raw?.manifest?.context_object_refs).length
                    )}
                    {kv(
                      "artifact refs",
                      toArray(selectedContextPack.raw?.manifest?.artifact_refs).length
                    )}
                    {kv(
                      "memory refs",
                      toArray(selectedContextPack.raw?.manifest?.governed_memory_refs).length
                    )}
                  </div>
                  <div className="grid gap-2">
                    <div className="font-medium text-slate-200">Bind history</div>
                    {selectedContextPack.bindings.length ? (
                      <div className="grid gap-2">
                        {selectedContextPack.bindings.map((binding: any) => (
                          <div
                            key={binding.binding_id}
                            className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                          >
                            <div className="flex flex-wrap items-center justify-between gap-2">
                              <div className="font-medium text-slate-100">
                                {binding.alias || binding.binding_id}
                              </div>
                              <div className="flex flex-wrap items-center gap-1">
                                <span
                                  className={
                                    binding.required ? "tcp-badge-warning" : "tcp-badge-info"
                                  }
                                >
                                  {binding.required ? "required" : "optional"}
                                </span>
                              </div>
                            </div>
                            <div className="mt-2 grid gap-2 sm:grid-cols-2">
                              {kv("consumer plan", binding.consumer_plan_id || "n/a")}
                              {kv("consumer project", binding.consumer_project_key || "n/a")}
                              {kv("consumer workspace", binding.consumer_workspace_root || "n/a")}
                              {kv("created", timestampLabel(binding.created_at_ms))}
                            </div>
                            {binding.actor_metadata ? (
                              <div className="mt-2">
                                <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                                  actor metadata
                                </div>
                                <LazyJson
                                  value={binding.actor_metadata}
                                  className="mt-1"
                                  preClassName="tcp-code mt-1 max-h-28 overflow-auto tcp-text-caption"
                                />
                              </div>
                            ) : null}
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 tcp-text-caption text-slate-400">
                        No bindings recorded on this shared workflow context yet.
                      </div>
                    )}
                  </div>
                </div>
              ) : (
                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-3 tcp-text-caption text-slate-400">
                  Select a shared workflow context to inspect its provenance and bind history.
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3 tcp-text-caption text-slate-400">
            No shared workflow contexts have been published for this workspace yet.
          </div>
        )}
        {suggestedContextPacks.length ? (
          <div className="rounded-lg border border-emerald-500/20 bg-emerald-500/5 p-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="font-medium text-emerald-100">
                Suggested recent shared workflow contexts
              </div>
              <span className="tcp-badge-info">copy only, no auto-bind</span>
            </div>
            <div className="mt-2 grid gap-2">
              {suggestedContextPacks.map((pack: any) => (
                <div
                  key={pack.packId}
                  className="rounded-md border border-emerald-500/20 bg-slate-950/30 p-2"
                >
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <div className="font-medium text-slate-100">{pack.title}</div>
                      <div className="tcp-subtle tcp-text-caption">{pack.reason}</div>
                    </div>
                    <div className="flex flex-wrap items-center gap-1">
                      <span className="tcp-badge-info">{pack.state}</span>
                      <button
                        type="button"
                        className="tcp-btn h-7 px-2 tcp-text-caption"
                        onClick={() => void copyContextPackId(pack.packId)}
                      >
                        <i data-lucide="copy"></i>
                        Copy context id
                      </button>
                    </div>
                  </div>
                  <div className="mt-2 grid gap-2 sm:grid-cols-2">
                    {kv("context id", pack.packId)}
                    {kv("source plan", pack.sourcePlanId || "n/a")}
                    {kv("updated", timestampLabel(pack.updatedAtMs))}
                    {kv("freshness", pack.freshnessWindowHours || "n/a")}
                  </div>
                </div>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
