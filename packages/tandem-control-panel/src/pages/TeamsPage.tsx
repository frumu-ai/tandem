import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { AgentStandupBuilder } from "./AgentStandupBuilder";
import { PageCard, EmptyState } from "./ui";
import { useCapabilities } from "../features/system/queries.ts";
import type { AppPageProps } from "./pageTypes";
import agentCatalog from "../generated/agent-catalog.json";
import { detectBrowserTimezone } from "../features/automations/timezone";
import { Icon } from "../ui/Icon";

type AgentCatalogCategory = {
  id: string;
  title: string;
  summary: string;
  source_path: string;
  count: number;
};

type AgentCatalogEntry = {
  id: string;
  name: string;
  summary: string;
  category_id: string;
  category_title: string;
  category_summary: string;
  source_path: string;
  source_file: string;
  sandbox_mode: string;
  target_surfaces: string[];
  instructions: string;
  tags: string[];
  requires: string[];
  role: string;
};

type AgentCatalogIndex = {
  generated_at: string;
  source_root: string;
  categories: AgentCatalogCategory[];
  agents: AgentCatalogEntry[];
};

const CATALOG = agentCatalog as AgentCatalogIndex;
const AGENT_CATALOG_HANDOFF_KEY = "tandem.studio.agentCatalogHandoff";

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

function normalizeQuery(value: string) {
  return String(value || "")
    .trim()
    .toLowerCase();
}

function previewInstructions(entry: AgentCatalogEntry) {
  const block = String(entry.instructions || "")
    .trim()
    .split(/\n\s*\n/)
    .find(Boolean)
    ?.trim();
  return block || entry.summary;
}

export function TeamsPage({ client, toast, navigate }: AppPageProps) {
  const queryClient = useQueryClient();
  const agentTeams = (client as any)?.agentTeams;
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [form, setForm] = useState<TemplateFormState>(EMPTY_FORM);
  const [editingTemplateId, setEditingTemplateId] = useState<string | null>(null);
  const [catalogQuery, setCatalogQuery] = useState("");
  const [catalogCategory, setCatalogCategory] = useState("all");
  const [catalogStatus, setCatalogStatus] = useState("");
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [timezone, setTimezone] = useState(() => detectBrowserTimezone());
  const avatarInputRef = useRef<HTMLInputElement | null>(null);
  const agentsSectionRef = useRef<HTMLDivElement | null>(null);
  const caps = useCapabilities();
  const agentTeamsAvailable = caps.data?.agent_teams === true;
  const healthQuery = useQuery({
    queryKey: ["teams", "health"],
    queryFn: () => client?.health?.().catch(() => ({})) ?? Promise.resolve({}),
    refetchInterval: 30000,
  });
  const defaultWorkspaceRoot = String(
    (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root || ""
  ).trim();
  const effectiveWorkspaceRoot = String(workspaceRoot || defaultWorkspaceRoot || "").trim();

  useEffect(() => {
    if (!defaultWorkspaceRoot) return;
    setWorkspaceRoot((current) => current || defaultWorkspaceRoot);
  }, [defaultWorkspaceRoot]);

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
  const filteredCatalogAgents = useMemo(() => {
    const query = normalizeQuery(catalogQuery);
    return CATALOG.agents.filter((entry) => {
      if (catalogCategory !== "all" && entry.category_id !== catalogCategory) {
        return false;
      }
      if (!query) return true;
      const haystack = [
        entry.name,
        entry.summary,
        entry.category_title,
        entry.category_id,
        entry.source_path,
        entry.source_file,
        entry.sandbox_mode,
        entry.role,
        ...(entry.tags || []),
        ...(entry.requires || []),
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(query);
    });
  }, [catalogCategory, catalogQuery]);

  const filteredCatalogCategories = useMemo(() => {
    const map = new Map<string, AgentCatalogEntry[]>();
    for (const entry of filteredCatalogAgents) {
      const current = map.get(entry.category_id) || [];
      current.push(entry);
      map.set(entry.category_id, current);
    }
    return CATALOG.categories
      .map((category) => ({
        ...category,
        agents: map.get(category.id) || [],
      }))
      .filter((category) => category.agents.length > 0);
  }, [filteredCatalogAgents]);

  const [copiedPath, setCopiedPath] = useState("");

  const copyCatalogPath = async (path: string) => {
    try {
      await navigator.clipboard.writeText(path);
      setCopiedPath(path);
      setCatalogStatus(`Copied: ${path}`);
      setTimeout(() => setCopiedPath((current) => (current === path ? "" : current)), 2000);
    } catch (error) {
      setCatalogStatus(error instanceof Error ? error.message : String(error));
    }
  };

  const seedStudioFromCatalog = (entry: AgentCatalogEntry) => {
    try {
      sessionStorage.setItem(
        AGENT_CATALOG_HANDOFF_KEY,
        JSON.stringify({
          agentId: entry.id,
          displayName: entry.name,
          categoryId: entry.category_id,
          categoryTitle: entry.category_title,
          summary: entry.summary,
          sourcePath: entry.source_path,
          sandboxMode: entry.sandbox_mode,
          role: entry.role,
          tags: entry.tags,
          requires: entry.requires,
          instructions: entry.instructions,
        })
      );
      // Open Studio in a new tab so the user doesn't lose their current form state.
      const base = window.location.href.replace(/#.*$/, "");
      window.open(`${base}#studio`, "_blank", "noopener");
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  const importFromCatalog = (entry: AgentCatalogEntry) => {
    // Derive a clean template ID from the catalog entry name.
    const derivedId = entry.name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "");
    // Map catalog role to the agent template role options.
    const roleMap: Record<string, TemplateFormState["role"]> = {
      worker: "worker",
      reviewer: "reviewer",
      delegator: "delegator",
      orchestrator: "orchestrator",
      watcher: "watcher",
      committer: "committer",
      tester: "tester",
    };
    const mappedRole: TemplateFormState["role"] = roleMap[entry.role] ?? "worker";
    setEditingTemplateId(null);
    setForm({
      templateId: derivedId,
      displayName: entry.name,
      avatarUrl: "",
      role: mappedRole,
      // Use the first paragraph of instructions as the system prompt so it's a
      // useful starting point the user can edit, not just the summary.
      systemPrompt: [
        entry.summary,
        previewInstructions(entry) !== entry.summary ? previewInstructions(entry) : "",
      ]
        .filter(Boolean)
        .join("\n\n")
        .trim(),
      modelProvider: "",
      modelId: "",
    });
    setCatalogStatus(`Imported "${entry.name}" into the agent form — review and save below.`);
    // Scroll the agent creation form into view.
    setTimeout(() => {
      agentsSectionRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
    }, 80);
  };

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
    <div ref={rootRef} className="grid gap-4">
      {agentTeamsAvailable && (
        <PageCard
          title="Agent Standup"
          subtitle="Compose scheduled standups from the same saved agents you manage here"
        >
          <AgentStandupBuilder
            client={client}
            toast={toast}
            workspaceRoot={effectiveWorkspaceRoot}
            onWorkspaceRootChange={setWorkspaceRoot}
            defaultWorkspaceRoot={defaultWorkspaceRoot}
            timezone={timezone}
            onTimezoneChange={setTimezone}
            templates={templates.map((template) => ({
              templateId: template.templateId,
              displayName: template.displayName || template.templateId,
              role: template.role,
              modelLabel:
                template.modelProvider && template.modelId
                  ? `${template.modelProvider}/${template.modelId}`
                  : "",
            }))}
          />
        </PageCard>
      )}

      <PageCard
        title="Agent Catalog"
        subtitle="Browse and import from the canonical Codex subagent set. Use any entry to pre-fill the agent form below."
      >
        <div className="grid gap-3">
          <div className="grid gap-2 md:grid-cols-[1fr_auto]">
            <input
              className="tcp-input"
              placeholder="Search agent name, category, tag, or path"
              value={catalogQuery}
              onInput={(event) => setCatalogQuery((event.target as HTMLInputElement).value)}
            />
            <button
              className="tcp-btn"
              onClick={() => {
                setCatalogQuery("");
                setCatalogCategory("all");
              }}
              disabled={!catalogQuery && catalogCategory === "all"}
            >
              Clear
            </button>
          </div>

          <div className="flex flex-wrap gap-2">
            <button
              className={`tcp-btn h-8 px-3 text-xs ${catalogCategory === "all" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
              onClick={() => setCatalogCategory("all")}
            >
              All
            </button>
            {CATALOG.categories.map((category) => (
              <button
                key={category.id}
                className={`tcp-btn h-8 px-3 text-xs ${catalogCategory === category.id ? "border-amber-400/60 bg-amber-400/10" : ""}`}
                onClick={() => setCatalogCategory(category.id)}
              >
                {category.title} ({category.count})
              </button>
            ))}
          </div>

          {catalogStatus ? <div className="text-xs text-slate-400">{catalogStatus}</div> : null}

          {filteredCatalogCategories.length ? (
            filteredCatalogCategories.map((category) => (
              <div key={category.id} className="grid gap-2">
                <div className="flex items-start justify-between gap-2">
                  <div>
                    <div className="font-medium text-slate-100">{category.title}</div>
                    <div className="text-xs text-slate-400">{category.summary}</div>
                  </div>
                  <div className="tcp-badge-info">{category.agents.length} agents</div>
                </div>
                <div className="grid gap-2 md:grid-cols-2">
                  {category.agents.map((entry) => (
                    <div key={entry.source_path} className="tcp-list-item grid gap-2">
                      <div className="flex items-start justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate font-semibold text-slate-100">{entry.name}</div>
                          <div className="text-xs text-slate-400">{entry.summary}</div>
                        </div>
                        <div className="flex flex-wrap gap-2">
                          <span className="tcp-badge-info">{entry.role}</span>
                          <span
                            className={
                              entry.sandbox_mode === "read-only" ? "tcp-badge-warn" : "tcp-badge-ok"
                            }
                          >
                            {entry.sandbox_mode}
                          </span>
                        </div>
                      </div>
                      <div className="font-mono text-xs text-slate-400">{entry.source_path}</div>
                      <div className="line-clamp-3 text-xs text-slate-200">
                        {previewInstructions(entry)}
                      </div>
                      <div className="flex flex-wrap gap-1">
                        {entry.tags.slice(0, 4).map((tag) => (
                          <span key={`${entry.id}-${tag}`} className="tcp-badge-muted">
                            {tag}
                          </span>
                        ))}
                        {entry.requires.slice(0, 3).map((req) => (
                          <span key={`${entry.id}-${req}`} className="tcp-badge-info">
                            {req}
                          </span>
                        ))}
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <button
                          className="tcp-btn h-8 px-3 text-xs"
                          title="Pre-fill the agent creation form from this catalog entry"
                          onClick={() => importFromCatalog(entry)}
                        >
                          <Icon name="download" />
                          Import as Agent
                        </button>
                        <button
                          className="tcp-btn h-8 px-3 text-xs"
                          title="Open this agent in Studio (new tab)"
                          onClick={() => seedStudioFromCatalog(entry)}
                        >
                          <Icon name="external-link" />
                          Studio ↗
                        </button>
                        <button
                          className={`tcp-btn h-8 px-3 text-xs ${
                            copiedPath === entry.source_path
                              ? "border-emerald-400/60 bg-emerald-400/10 text-emerald-300"
                              : ""
                          }`}
                          title="Copy source path to clipboard"
                          onClick={() => void copyCatalogPath(entry.source_path)}
                        >
                          <Icon name={copiedPath === entry.source_path ? "check" : "copy"} />
                          {copiedPath === entry.source_path ? "Copied" : "Copy path"}
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ))
          ) : (
            <EmptyState text="No agents match your search." />
          )}
        </div>
      </PageCard>

      <div className="grid gap-4 xl:grid-cols-2">
        <div ref={agentsSectionRef}>
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
                  Each saved agent defines a persistent personality for automation workflows. Define
                  who the agent is, what kind of work it owns, and which default model it should
                  use. These personalities can be reused in standups and other workflow responses.
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
                    describe ownership and judgment: frontend lead, backend lead, product ops,
                    incident watcher.
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    {PROMPT_EXAMPLES.map((example) => (
                      <button
                        type="button"
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
                <div className="border border-slate-800/80 bg-[radial-gradient(circle_at_top,_rgba(34,211,238,0.18),_transparent_45%),linear-gradient(180deg,rgba(15,23,42,0.9),rgba(2,6,23,0.96))] p-5">
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
                        type="button"
                        className="tcp-icon-btn"
                        title="Upload avatar"
                        aria-label="Upload avatar"
                        onClick={() => avatarInputRef.current?.click()}
                      >
                        <Icon name="pencil" />
                      </button>
                      <button
                        type="button"
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
                        <Icon name="trash-2" />
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
                  type="button"
                  className="tcp-btn"
                  disabled={templateMutation.isPending}
                  onClick={() => templateMutation.mutate()}
                >
                  <Icon name="save" />
                  {editingTemplateId ? "Update Agent" : "Create Agent"}
                </button>
                {hasDraft && (
                  <button
                    type="button"
                    className="tcp-btn"
                    onClick={() => {
                      setEditingTemplateId(null);
                      setForm(EMPTY_FORM);
                    }}
                  >
                    <Icon name="rotate-ccw" />
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
                            type="button"
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
                            <Icon name="pencil" />
                            Edit
                          </button>
                          <button
                            type="button"
                            className="tcp-btn-danger h-7 px-2 text-xs"
                            onClick={() => deleteMutation.mutate(template.templateId)}
                          >
                            <Icon name="trash-2" />
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
        </div>

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
                  approval?.approval_id ||
                    approval?.request_id ||
                    approval?.id ||
                    `request-${index}`
                );
                return (
                  <div key={requestId} className="tcp-list-item">
                    <div className="mb-1 font-medium">
                      {String(approval?.reason || approval?.title || requestId)}
                    </div>
                    <div className="tcp-subtle text-xs">{requestId}</div>
                    <div className="mt-2 flex gap-2">
                      <button
                        type="button"
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() => replyMutation.mutate({ requestId, decision: "approve" })}
                      >
                        <Icon name="badge-check" />
                        Approve
                      </button>
                      <button
                        type="button"
                        className="tcp-btn-danger h-7 px-2 text-xs"
                        onClick={() => replyMutation.mutate({ requestId, decision: "deny" })}
                      >
                        <Icon name="x" />
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
    </div>
  );
}
