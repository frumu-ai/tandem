import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useRef, useState } from "react";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

const ROLE_OPTIONS = [
  "worker",
  "reviewer",
  "tester",
  "watcher",
  "delegator",
  "committer",
  "orchestrator",
] as const;

const ROLE_HINTS: Record<(typeof ROLE_OPTIONS)[number], string> = {
  worker: "Executes hands-on work and reports concrete progress.",
  reviewer: "Critiques output, spots risks, and improves quality.",
  tester: "Validates behavior and looks for regressions or gaps.",
  watcher: "Monitors activity, incidents, and changes over time.",
  delegator: "Breaks work down and routes tasks across participants.",
  committer: "Finalizes work and drives it toward completion.",
  orchestrator: "Coordinates multi-agent flow and synthesizes updates.",
};

const PROMPT_EXAMPLES = [
  "You are the frontend lead. Focus on shipped UI changes, active branches, visual regressions, and blockers from design or review.",
  "You are the backend lead. Focus on APIs, database work, deploys, incidents, and blockers from reliability or dependencies.",
  "You are the product and ops agent. Focus on launches, customer feedback, analytics, triage, and operational blockers.",
];

interface TemplateFormState {
  templateId: string;
  displayName: string;
  avatarUrl: string;
  role: (typeof ROLE_OPTIONS)[number];
  systemPrompt: string;
  modelProvider: string;
  modelId: string;
}

const EMPTY_FORM: TemplateFormState = {
  templateId: "",
  displayName: "",
  avatarUrl: "",
  role: "worker",
  systemPrompt: "",
  modelProvider: "",
  modelId: "",
};

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function normalizeTemplate(row: any) {
  const defaultModel = row?.default_model || row?.defaultModel || {};
  return {
    templateId: String(row?.template_id || row?.templateID || row?.id || "").trim(),
    displayName: String(row?.display_name || row?.displayName || row?.name || "").trim(),
    avatarUrl: String(row?.avatar_url || row?.avatarUrl || "").trim(),
    role: String(row?.role || "worker").trim() || "worker",
    systemPrompt: String(row?.system_prompt || row?.systemPrompt || "").trim(),
    modelProvider: String(defaultModel?.provider_id || defaultModel?.providerId || "").trim(),
    modelId: String(defaultModel?.model_id || defaultModel?.modelId || "").trim(),
  };
}

function buildTemplatePayload(form: TemplateFormState) {
  const template: Record<string, unknown> = {
    templateID: form.templateId.trim(),
    display_name: form.displayName.trim() || undefined,
    avatar_url: form.avatarUrl.trim() || undefined,
    role: form.role,
    system_prompt: form.systemPrompt.trim() || undefined,
    skills: [],
    default_budget: {},
    capabilities: {},
  };
  if (form.modelProvider.trim() && form.modelId.trim()) {
    template.default_model = {
      provider_id: form.modelProvider.trim(),
      model_id: form.modelId.trim(),
    };
  }
  return template;
}

export function TeamsPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const agentTeams = (client as any)?.agentTeams;
  const [form, setForm] = useState<TemplateFormState>(EMPTY_FORM);
  const [editingTemplateId, setEditingTemplateId] = useState<string | null>(null);
  const avatarInputRef = useRef<HTMLInputElement | null>(null);

  const templatesQuery = useQuery({
    queryKey: ["teams", "templates"],
    queryFn: () =>
      agentTeams?.listTemplates?.().catch(() => ({ templates: [] })) ??
      Promise.resolve({ templates: [] }),
    refetchInterval: 8000,
  });
  const instancesQuery = useQuery({
    queryKey: ["teams", "instances"],
    queryFn: () =>
      agentTeams?.listInstances?.().catch(() => ({ instances: [] })) ??
      Promise.resolve({ instances: [] }),
    refetchInterval: 8000,
  });
  const approvalsQuery = useQuery({
    queryKey: ["teams", "approvals"],
    queryFn: () =>
      agentTeams?.listApprovals?.().catch(() => ({ spawnApprovals: [] })) ??
      Promise.resolve({ spawnApprovals: [] }),
    refetchInterval: 6000,
  });

  const templateMutation = useMutation({
    mutationFn: async () => {
      const templateId = form.templateId.trim();
      if (!templateId) throw new Error("Template ID is required.");
      if (editingTemplateId) {
        return agentTeams?.updateTemplate?.(editingTemplateId, {
          display_name: form.displayName.trim() || undefined,
          avatar_url: form.avatarUrl.trim() || undefined,
          role: form.role,
          system_prompt: form.systemPrompt.trim() || undefined,
          default_model:
            form.modelProvider.trim() && form.modelId.trim()
              ? {
                  provider_id: form.modelProvider.trim(),
                  model_id: form.modelId.trim(),
                }
              : undefined,
        } as any);
      }
      return agentTeams?.createTemplate?.({
        template: buildTemplatePayload(form),
      } as any);
    },
    onSuccess: async () => {
      toast("ok", editingTemplateId ? "Template updated." : "Template created.");
      setForm(EMPTY_FORM);
      setEditingTemplateId(null);
      await queryClient.invalidateQueries({ queryKey: ["teams"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const deleteMutation = useMutation({
    mutationFn: (templateId: string) => agentTeams?.deleteTemplate?.(templateId),
    onSuccess: async () => {
      toast("ok", "Template deleted.");
      if (editingTemplateId) {
        setEditingTemplateId(null);
        setForm(EMPTY_FORM);
      }
      await queryClient.invalidateQueries({ queryKey: ["teams"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const replyMutation = useMutation({
    mutationFn: ({ requestId, decision }: { requestId: string; decision: "approve" | "deny" }) =>
      decision === "approve"
        ? agentTeams?.approveSpawn?.(requestId)
        : agentTeams?.denySpawn?.(requestId),
    onSuccess: async () => {
      toast("ok", "Approval updated.");
      await queryClient.invalidateQueries({ queryKey: ["teams"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const templates = useMemo(
    () =>
      toArray(templatesQuery.data, "templates")
        .map(normalizeTemplate)
        .filter((row) => row.templateId),
    [templatesQuery.data]
  );
  const instances = toArray(instancesQuery.data, "instances");
  const approvals = toArray(approvalsQuery.data, "spawnApprovals");
  const hasDraft = !!(
    editingTemplateId ||
    form.templateId ||
    form.displayName ||
    form.systemPrompt
  );
  const personalityName = form.displayName.trim() || form.templateId.trim() || "New Agent";
  const personalityInitial = personalityName.slice(0, 1).toUpperCase() || "A";
  const selectedRoleHint = ROLE_HINTS[form.role];
  const avatarUrl = form.avatarUrl.trim();

  const handleAvatarUpload = (file: File | null) => {
    if (!file) return;
    if (file.size > 10 * 1024 * 1024) {
      toast("err", "Avatar image is too large (max 10 MB).");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const value = typeof reader.result === "string" ? reader.result : "";
      if (!value) {
        toast("err", "Failed to read avatar image.");
        return;
      }
      setForm((current) => ({
        ...current,
        avatarUrl: value,
      }));
    };
    reader.onerror = () => toast("err", "Failed to read avatar image.");
    reader.readAsDataURL(file);
  };

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <PageCard
        title="Agents"
        subtitle="Create reusable agent personalities, prompts, and default models for automation workflows"
      >
        <div className="grid gap-3">
          <div className="rounded-2xl border border-cyan-500/20 bg-cyan-500/5 p-4">
            <div className="text-xs font-medium uppercase tracking-[0.24em] text-cyan-300">
              Reusable Personalities
            </div>
            <div className="mt-2 text-sm text-slate-300">
              Each saved agent defines a persistent personality for automation workflows. Define who
              the agent is, what kind of work it owns, and which default model it should use. These
              personalities can be reused in standups and other workflow responses.
            </div>
          </div>
          <div className="grid gap-2 md:grid-cols-2">
            <input
              className="tcp-input"
              placeholder="template-id"
              value={form.templateId}
              disabled={!!editingTemplateId}
              onInput={(event) =>
                setForm((current) => ({
                  ...current,
                  templateId: (event.target as HTMLInputElement).value,
                }))
              }
            />
            <input
              className="tcp-input"
              placeholder="Display name"
              value={form.displayName}
              onInput={(event) =>
                setForm((current) => ({
                  ...current,
                  displayName: (event.target as HTMLInputElement).value,
                }))
              }
            />
            <select
              className="tcp-input"
              value={form.role}
              onInput={(event) =>
                setForm((current) => ({
                  ...current,
                  role: (event.target as HTMLSelectElement).value as TemplateFormState["role"],
                }))
              }
            >
              {ROLE_OPTIONS.map((role) => (
                <option key={role} value={role}>
                  {role}
                </option>
              ))}
            </select>
            <input
              className="tcp-input"
              placeholder="Avatar URL or upload (optional)"
              value={form.avatarUrl}
              onInput={(event) =>
                setForm((current) => ({
                  ...current,
                  avatarUrl: (event.target as HTMLInputElement).value,
                }))
              }
            />
            <input
              className="tcp-input"
              placeholder="Model provider (optional)"
              value={form.modelProvider}
              onInput={(event) =>
                setForm((current) => ({
                  ...current,
                  modelProvider: (event.target as HTMLInputElement).value,
                }))
              }
            />
            <input
              className="tcp-input"
              placeholder="Model ID (optional)"
              value={form.modelId}
              onInput={(event) =>
                setForm((current) => ({
                  ...current,
                  modelId: (event.target as HTMLInputElement).value,
                }))
              }
            />
          </div>
          <div className="grid gap-3 lg:grid-cols-[1.15fr_0.85fr]">
            <div className="rounded-2xl border border-slate-800/80 bg-slate-950/40 px-4 py-3">
              <div className="text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
                Prompt Guidance
              </div>
              <div className="mt-2 text-sm text-slate-300">
                Write the lasting perspective for this agent, not a one-off task. Good prompts
                describe ownership and judgment: frontend lead, backend lead, product ops, incident
                watcher.
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                {PROMPT_EXAMPLES.map((example) => (
                  <button
                    key={example}
                    className="tcp-btn h-auto min-h-8 px-3 py-2 text-left text-xs"
                    onClick={() =>
                      setForm((current) => ({
                        ...current,
                        systemPrompt: example,
                      }))
                    }
                  >
                    Use Example
                  </button>
                ))}
              </div>
            </div>
            <div className="rounded-[28px] border border-slate-800/80 bg-[radial-gradient(circle_at_top,_rgba(34,211,238,0.18),_transparent_45%),linear-gradient(180deg,rgba(15,23,42,0.9),rgba(2,6,23,0.96))] p-5">
              <div className="flex items-start justify-between gap-4">
                <div className="flex items-start gap-4">
                  <div className="flex h-14 w-14 items-center justify-center overflow-hidden rounded-2xl border border-cyan-400/30 bg-cyan-400/10 text-lg font-semibold text-cyan-100">
                    {avatarUrl ? (
                      <img
                        src={avatarUrl}
                        alt={personalityName}
                        className="h-full w-full object-cover"
                      />
                    ) : (
                      personalityInitial
                    )}
                  </div>
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <strong className="text-white">{personalityName}</strong>
                      <span className="tcp-badge-info">{form.role}</span>
                    </div>
                    <div className="mt-1 text-xs text-slate-400">
                      {form.templateId.trim() || "template-id"}
                    </div>
                    <div className="mt-2 text-sm text-slate-300">{selectedRoleHint}</div>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <button
                    className="tcp-icon-btn"
                    title="Upload avatar"
                    aria-label="Upload avatar"
                    onClick={() => avatarInputRef.current?.click()}
                  >
                    <i data-lucide="pencil"></i>
                  </button>
                  <button
                    className="tcp-icon-btn"
                    title="Clear avatar"
                    aria-label="Clear avatar"
                    onClick={() =>
                      setForm((current) => ({
                        ...current,
                        avatarUrl: "",
                      }))
                    }
                  >
                    <i data-lucide="trash-2"></i>
                  </button>
                </div>
              </div>
              <div className="mt-3 text-xs text-slate-400">
                Upload an image like Settings Identity preview, or paste a direct avatar URL.
              </div>
              <input
                ref={avatarInputRef}
                type="file"
                accept="image/*"
                className="hidden"
                onChange={(event) => {
                  handleAvatarUpload((event.target as HTMLInputElement).files?.[0] || null);
                  (event.target as HTMLInputElement).value = "";
                }}
              />
              <div className="mt-4 rounded-2xl border border-slate-800/70 bg-black/20 p-4">
                <div className="text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
                  Personality Preview
                </div>
                <div className="mt-2 whitespace-pre-wrap text-sm leading-6 text-slate-200">
                  {form.systemPrompt.trim() ||
                    "This agent will respond from the persistent personality you define here across workflows and standups."}
                </div>
              </div>
              {(form.modelProvider.trim() || form.modelId.trim()) && (
                <div className="mt-3 text-xs text-cyan-200">
                  Default model: {form.modelProvider.trim() || "provider"}/
                  {form.modelId.trim() || "model"}
                </div>
              )}
            </div>
          </div>
          <textarea
            className="tcp-input min-h-[140px]"
            placeholder="Persistent system prompt"
            value={form.systemPrompt}
            onInput={(event) =>
              setForm((current) => ({
                ...current,
                systemPrompt: (event.target as HTMLTextAreaElement).value,
              }))
            }
          />
          <div className="flex flex-wrap gap-2">
            <button
              className="tcp-btn"
              disabled={templateMutation.isPending}
              onClick={() => templateMutation.mutate()}
            >
              <i data-lucide="save"></i>
              {editingTemplateId ? "Update Agent" : "Create Agent"}
            </button>
            {hasDraft && (
              <button
                className="tcp-btn"
                onClick={() => {
                  setEditingTemplateId(null);
                  setForm(EMPTY_FORM);
                }}
              >
                <i data-lucide="rotate-ccw"></i>
                Reset
              </button>
            )}
          </div>
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <div className="text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
                Saved Agents
              </div>
              <div className="tcp-subtle text-xs">{templates.length} saved</div>
            </div>
            {templates.length ? (
              templates.map((template) => (
                <div key={template.templateId} className="tcp-list-item">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <strong>{template.displayName || template.templateId}</strong>
                        <span className="tcp-badge-info">{template.role}</span>
                        {template.modelProvider && template.modelId ? (
                          <span className="tcp-badge-ok">
                            {template.modelProvider}/{template.modelId}
                          </span>
                        ) : null}
                      </div>
                      <div className="tcp-subtle text-xs">{template.templateId}</div>
                      {template.systemPrompt ? (
                        <div className="mt-2 line-clamp-4 text-xs text-slate-300">
                          {template.systemPrompt}
                        </div>
                      ) : (
                        <div className="mt-2 text-xs text-slate-500">
                          No persistent prompt set yet.
                        </div>
                      )}
                    </div>
                    <div className="flex gap-2">
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() => {
                          setEditingTemplateId(template.templateId);
                          setForm({
                            templateId: template.templateId,
                            displayName: template.displayName,
                            avatarUrl: template.avatarUrl,
                            role: (ROLE_OPTIONS.includes(template.role as any)
                              ? template.role
                              : "worker") as TemplateFormState["role"],
                            systemPrompt: template.systemPrompt,
                            modelProvider: template.modelProvider,
                            modelId: template.modelId,
                          });
                        }}
                      >
                        <i data-lucide="pencil"></i>
                        Edit
                      </button>
                      <button
                        className="tcp-btn-danger h-7 px-2 text-xs"
                        onClick={() => deleteMutation.mutate(template.templateId)}
                      >
                        <i data-lucide="trash-2"></i>
                        Delete
                      </button>
                    </div>
                  </div>
                </div>
              ))
            ) : (
              <EmptyState
                title="No agents yet"
                text="Create your first saved personality here, then reuse it across automation workflows and standups."
              />
            )}
          </div>
        </div>
      </PageCard>

      <PageCard title="Team Instances" subtitle="Running collaborative agent instances">
        <div className="grid gap-2">
          {instances.length ? (
            instances.map((instance: any, index: number) => (
              <div
                key={String(instance?.instance_id || instance?.id || index)}
                className="tcp-list-item"
              >
                <div className="mb-1 flex items-center justify-between gap-2">
                  <strong>
                    {String(
                      instance?.template_id ||
                        instance?.templateID ||
                        instance?.instance_id ||
                        "Instance"
                    )}
                  </strong>
                  <span className="tcp-badge-info">{String(instance?.status || "active")}</span>
                </div>
                <div className="tcp-subtle text-xs">
                  mission: {String(instance?.mission_id || instance?.missionID || "n/a")}
                </div>
              </div>
            ))
          ) : (
            <EmptyState text="No team instances found." />
          )}
        </div>
      </PageCard>

      <PageCard title="Spawn Approvals" subtitle="Pending team approval requests">
        <div className="grid gap-2">
          {approvals.length ? (
            approvals.map((approval: any, index: number) => {
              const requestId = String(
                approval?.approval_id || approval?.request_id || approval?.id || `request-${index}`
              );
              return (
                <div key={requestId} className="tcp-list-item">
                  <div className="mb-1 font-medium">
                    {String(approval?.reason || approval?.title || requestId)}
                  </div>
                  <div className="tcp-subtle text-xs">{requestId}</div>
                  <div className="mt-2 flex gap-2">
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => replyMutation.mutate({ requestId, decision: "approve" })}
                    >
                      <i data-lucide="badge-check"></i>
                      Approve
                    </button>
                    <button
                      className="tcp-btn-danger h-7 px-2 text-xs"
                      onClick={() => replyMutation.mutate({ requestId, decision: "deny" })}
                    >
                      <i data-lucide="x"></i>
                      Deny
                    </button>
                  </div>
                </div>
              );
            })
          ) : (
            <EmptyState text="No pending approvals." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
