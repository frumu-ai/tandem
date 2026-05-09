import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { renderIcons } from "../../app/icons.js";
import { ConfirmDialog } from "../../components/ControlPanelDialogs";
import { Badge, PanelCard, Toolbar } from "../../ui/index.tsx";

type PromptCollectionOverride = {
  collection_id: string;
  updated_at: string;
};

type PromptEntry = {
  key: string;
  description: string;
  supports_collection_override: boolean;
  default: string;
  global_override: string | null;
  global_override_updated_at: string | null;
  collection_override: string | null;
  collection_override_updated_at: string | null;
  current: string;
  scope: "default" | "global" | "collection";
  requested_collection_id: string | null;
  collection_overrides: PromptCollectionOverride[];
};

type PromptListResponse = {
  requested_collection_id: string | null;
  prompts: PromptEntry[];
};

type KnowledgebaseCollection = {
  collection_id: string;
};

const SCOPE_GLOBAL = "__global__";

function scopeLabel(scope: PromptEntry["scope"]): string {
  if (scope === "collection") return "Collection override";
  if (scope === "global") return "Global override";
  return "Default";
}

function scopeBadgeTone(scope: PromptEntry["scope"]): "ok" | "info" | "warn" {
  if (scope === "collection") return "ok";
  if (scope === "global") return "info";
  return "warn";
}

export function KnowledgebasePromptsPanel({
  api,
  toast,
}: {
  api: (path: string, init?: RequestInit) => Promise<any>;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
}) {
  const queryClient = useQueryClient();
  const [collapsed, setCollapsed] = useState(true);
  const [scope, setScope] = useState<string>(SCOPE_GLOBAL);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [showDefaults, setShowDefaults] = useState<Record<string, boolean>>({});
  const [resetConfirm, setResetConfirm] = useState<{
    key: string;
    collectionId: string | null;
  } | null>(null);

  const kbConfigQuery = useQuery({
    queryKey: ["knowledgebase", "config"],
    queryFn: async () => api("/api/knowledgebase/config").catch(() => ({ configured: false })),
    staleTime: 60_000,
  });
  const knowledgebaseAvailable = kbConfigQuery.data?.configured === true;

  const collectionsQuery = useQuery({
    queryKey: ["knowledgebase", "collections"],
    enabled: knowledgebaseAvailable,
    queryFn: async () => api("/api/knowledgebase/collections").catch(() => ({ collections: [] })),
    staleTime: 30_000,
  });

  const collections: KnowledgebaseCollection[] = Array.isArray(collectionsQuery.data?.collections)
    ? collectionsQuery.data.collections
    : [];

  const requestedCollection = scope === SCOPE_GLOBAL ? "" : scope;

  const promptsQuery = useQuery<PromptListResponse>({
    queryKey: ["knowledgebase", "prompts", requestedCollection || "__global__"],
    enabled: knowledgebaseAvailable && !collapsed,
    queryFn: async () => {
      const qs = requestedCollection
        ? `?collection_id=${encodeURIComponent(requestedCollection)}`
        : "";
      const data = await api(`/api/knowledgebase/prompts${qs}`);
      return data as PromptListResponse;
    },
  });

  const prompts: PromptEntry[] = promptsQuery.data?.prompts || [];

  // When prompts load or scope changes, seed drafts with the *current effective* values
  // for the selected scope (so editing in the textarea starts from what the agent sees).
  // Depend on dataUpdatedAt (a stable React Query timestamp) rather than the prompts
  // array — the array gets a new reference each render but its contents only change
  // when the query actually refetches.
  useEffect(() => {
    const fresh = promptsQuery.data?.prompts || [];
    if (!fresh.length) return;
    setDrafts((prev) => {
      const next: Record<string, string> = { ...prev };
      for (const entry of fresh) {
        // Initialize draft only if untouched
        if (!(entry.key in next)) {
          next[entry.key] =
            scope === SCOPE_GLOBAL
              ? (entry.global_override ?? entry.default)
              : (entry.collection_override ?? entry.global_override ?? entry.default);
        }
      }
      return next;
    });
  }, [promptsQuery.dataUpdatedAt, scope]);

  // Reset drafts whenever scope changes — owners likely want a clean slate per scope
  useEffect(() => {
    setDrafts({});
  }, [scope]);

  const setPromptMutation = useMutation({
    mutationFn: async ({
      key,
      value,
      collection_id,
    }: {
      key: string;
      value: string;
      collection_id?: string;
    }) => {
      const body: { value: string; collection_id?: string } = { value };
      if (collection_id) body.collection_id = collection_id;
      return api(`/api/knowledgebase/prompts/${encodeURIComponent(key)}`, {
        method: "PUT",
        body: JSON.stringify(body),
      });
    },
    onSuccess: async (_data, variables) => {
      toast("ok", `Saved prompt "${variables.key}".`);
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "prompts"] });
      setDrafts((prev) => {
        const copy = { ...prev };
        delete copy[variables.key];
        return copy;
      });
    },
    onError: (error: any) => {
      const message = error?.message || "Save failed.";
      toast("err", `Save failed: ${message}`);
    },
  });

  const deletePromptMutation = useMutation({
    mutationFn: async ({ key, collection_id }: { key: string; collection_id?: string }) => {
      const qs = collection_id ? `?collection_id=${encodeURIComponent(collection_id)}` : "";
      return api(`/api/knowledgebase/prompts/${encodeURIComponent(key)}${qs}`, {
        method: "DELETE",
      });
    },
    onSuccess: async (_data, variables) => {
      toast(
        "ok",
        variables.collection_id
          ? `Reverted "${variables.key}" for collection "${variables.collection_id}".`
          : `Reverted global "${variables.key}" to default.`
      );
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "prompts"] });
      setDrafts((prev) => {
        const copy = { ...prev };
        delete copy[variables.key];
        return copy;
      });
    },
    onError: (error: any) => {
      const message = error?.message || "Reset failed.";
      toast("err", `Reset failed: ${message}`);
    },
  });

  const collectionOptions = useMemo(() => {
    return collections
      .map((c) => String(c.collection_id || "").trim())
      .filter(Boolean)
      .sort();
  }, [collections]);

  if (!knowledgebaseAvailable) return null;

  return (
    <PanelCard
      title="Agent prompts"
      subtitle="Edit how the AI agent answers questions from this knowledgebase. Defaults are shipped in code; overrides take effect on the next agent session or answer_question call."
      actions={
        <Toolbar className="justify-end">
          <button
            type="button"
            className="tcp-btn h-8 w-8 justify-center px-0"
            title={collapsed ? "Expand prompts" : "Collapse prompts"}
            aria-label={collapsed ? "Expand prompts" : "Collapse prompts"}
            onClick={() => setCollapsed((value) => !value)}
            ref={(el) => {
              if (el) renderIcons(el);
            }}
          >
            <i data-lucide={collapsed ? "chevron-down" : "chevron-up"} aria-hidden="true" />
          </button>
        </Toolbar>
      }
    >
      {collapsed ? null : (
        <div className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center gap-3 rounded-lg border border-white/10 bg-black/20 p-3 text-sm">
            <label className="flex items-center gap-2">
              <span className="tcp-subtle">Scope:</span>
              <select
                className="tcp-input h-8 px-2"
                value={scope}
                onChange={(event) => setScope(event.target.value)}
              >
                <option value={SCOPE_GLOBAL}>Global (all collections)</option>
                {collectionOptions.map((cid) => (
                  <option key={cid} value={cid}>
                    Collection: {cid}
                  </option>
                ))}
              </select>
            </label>
            <span className="tcp-subtle text-xs">
              {scope === SCOPE_GLOBAL
                ? "Global overrides apply to every collection unless a collection-specific override exists."
                : `Editing overrides for collection "${scope}". Collection overrides take precedence over global ones.`}
            </span>
          </div>

          {promptsQuery.isLoading ? (
            <div className="tcp-subtle p-4 text-sm">Loading prompts…</div>
          ) : promptsQuery.isError ? (
            <div className="rounded-lg border border-rose-500/30 bg-rose-950/20 p-3 text-sm text-rose-200">
              Failed to load prompts: {(promptsQuery.error as any)?.message || "Unknown error."}
            </div>
          ) : prompts.length === 0 ? (
            <div className="tcp-subtle p-4 text-sm">No prompts available.</div>
          ) : (
            <div className="flex flex-col gap-3">
              {prompts.map((entry) => {
                const isExpanded = !!expanded[entry.key];
                const draft = drafts[entry.key] ?? entry.current;
                const editingDisabled =
                  scope !== SCOPE_GLOBAL && !entry.supports_collection_override;
                const canResetCurrentScope =
                  scope === SCOPE_GLOBAL
                    ? entry.global_override !== null
                    : entry.collection_override !== null;
                const showingDefault = !!showDefaults[entry.key];
                const dirty =
                  draft !==
                  (scope === SCOPE_GLOBAL
                    ? (entry.global_override ?? entry.default)
                    : (entry.collection_override ?? entry.global_override ?? entry.default));

                return (
                  <div
                    key={entry.key}
                    className="rounded-lg border border-white/10 bg-black/20 p-3"
                  >
                    <div className="flex flex-wrap items-start justify-between gap-2">
                      <div className="flex flex-col">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-sm">{entry.key}</span>
                          <Badge tone={scopeBadgeTone(entry.scope)}>
                            {scopeLabel(entry.scope)}
                          </Badge>
                          {!entry.supports_collection_override ? (
                            <Badge tone="warn">Global only</Badge>
                          ) : null}
                          {entry.collection_overrides.length > 0 ? (
                            <span className="tcp-subtle text-xs">
                              {entry.collection_overrides.length} collection override
                              {entry.collection_overrides.length === 1 ? "" : "s"}
                            </span>
                          ) : null}
                        </div>
                        <div className="tcp-subtle mt-1 text-xs">{entry.description}</div>
                      </div>
                      <Toolbar className="justify-end">
                        <button
                          type="button"
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() =>
                            setExpanded((prev) => ({
                              ...prev,
                              [entry.key]: !prev[entry.key],
                            }))
                          }
                        >
                          {isExpanded ? "Collapse" : "Edit"}
                        </button>
                      </Toolbar>
                    </div>

                    {isExpanded ? (
                      <div className="mt-3 flex flex-col gap-2">
                        {editingDisabled ? (
                          <div className="rounded border border-amber-500/30 bg-amber-950/20 p-2 text-xs text-amber-200">
                            This prompt is global only — it is sent before any collection is
                            selected, so per-collection overrides are not supported. Switch scope
                            back to Global to edit it.
                          </div>
                        ) : null}

                        <div className="flex items-center gap-2">
                          <button
                            type="button"
                            className="tcp-btn h-7 px-2 text-xs"
                            onClick={() =>
                              setShowDefaults((prev) => ({
                                ...prev,
                                [entry.key]: !prev[entry.key],
                              }))
                            }
                          >
                            {showingDefault ? "Hide default" : "Show default"}
                          </button>
                          {entry.global_override !== null && scope !== SCOPE_GLOBAL ? (
                            <span className="tcp-subtle text-xs">
                              Inherits a global override when no collection override is set.
                            </span>
                          ) : null}
                        </div>

                        {showingDefault ? (
                          <div className="rounded border border-white/10 bg-black/30 p-2">
                            <div className="tcp-subtle text-[10px] uppercase tracking-wide">
                              Built-in default
                            </div>
                            <pre className="whitespace-pre-wrap break-words font-mono text-xs leading-5">
                              {entry.default}
                            </pre>
                          </div>
                        ) : null}

                        <textarea
                          className="tcp-input min-h-[200px] resize-y font-mono text-xs leading-6"
                          value={draft}
                          disabled={editingDisabled}
                          onChange={(event) =>
                            setDrafts((prev) => ({
                              ...prev,
                              [entry.key]: event.target.value,
                            }))
                          }
                        />

                        <div className="flex flex-wrap items-center gap-2">
                          <button
                            type="button"
                            className="tcp-btn h-8 px-3 text-xs"
                            disabled={
                              editingDisabled ||
                              !dirty ||
                              !draft.trim() ||
                              setPromptMutation.isPending
                            }
                            onClick={() =>
                              setPromptMutation.mutate({
                                key: entry.key,
                                value: draft,
                                collection_id: scope === SCOPE_GLOBAL ? undefined : scope,
                              })
                            }
                          >
                            Save {scope === SCOPE_GLOBAL ? "global" : "collection"} override
                          </button>
                          <button
                            type="button"
                            className="tcp-btn h-8 px-3 text-xs"
                            disabled={
                              editingDisabled ||
                              !canResetCurrentScope ||
                              deletePromptMutation.isPending
                            }
                            onClick={() =>
                              setResetConfirm({
                                key: entry.key,
                                collectionId: scope === SCOPE_GLOBAL ? null : scope,
                              })
                            }
                          >
                            Reset {scope === SCOPE_GLOBAL ? "global" : "collection"} to default
                          </button>
                          {dirty ? (
                            <button
                              type="button"
                              className="tcp-btn h-8 px-3 text-xs"
                              onClick={() =>
                                setDrafts((prev) => {
                                  const copy = { ...prev };
                                  delete copy[entry.key];
                                  return copy;
                                })
                              }
                            >
                              Discard changes
                            </button>
                          ) : null}
                        </div>

                        {entry.collection_overrides.length > 0 ? (
                          <div className="mt-1 rounded border border-white/10 bg-black/30 p-2 text-xs">
                            <div className="tcp-subtle uppercase tracking-wide">
                              Collections with overrides
                            </div>
                            <ul className="mt-1 flex flex-wrap gap-2">
                              {entry.collection_overrides.map((override) => (
                                <li key={override.collection_id}>
                                  <button
                                    type="button"
                                    className="tcp-btn h-6 px-2 text-[11px]"
                                    onClick={() => setScope(override.collection_id)}
                                  >
                                    {override.collection_id}
                                  </button>
                                </li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                      </div>
                    ) : null}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
      <ConfirmDialog
        open={resetConfirm !== null}
        title="Reset prompt to default?"
        message={
          resetConfirm ? (
            <span>
              {resetConfirm.collectionId ? (
                <>
                  This removes the override for <code>{resetConfirm.key}</code> on collection{" "}
                  <code>{resetConfirm.collectionId}</code>. The agent will fall back to the global
                  override (if any) or the built-in default for this collection. Other collections
                  are unaffected.
                </>
              ) : (
                <>
                  This removes the global override for <code>{resetConfirm.key}</code>. The agent
                  will fall back to the built-in default. Per-collection overrides for this prompt
                  remain in place.
                </>
              )}
            </span>
          ) : (
            ""
          )
        }
        confirmLabel="Reset"
        confirmIcon="rotate-ccw"
        confirmTone="danger"
        confirmDisabled={deletePromptMutation.isPending}
        onCancel={() => setResetConfirm(null)}
        onConfirm={() => {
          if (!resetConfirm) return;
          deletePromptMutation.mutate({
            key: resetConfirm.key,
            collection_id: resetConfirm.collectionId ?? undefined,
          });
          setResetConfirm(null);
        }}
      />
    </PanelCard>
  );
}
