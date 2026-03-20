import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { EmptyState } from "./ui";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function statusTone(status: string) {
  const value = String(status || "")
    .trim()
    .toLowerCase();
  if (value === "running") return "tcp-badge-warn";
  if (value.includes("paused")) return "tcp-badge-info";
  if (value === "completed") return "tcp-badge-ok";
  if (value === "failed") return "tcp-badge-err";
  if (value.includes("approval")) return "tcp-badge-warn";
  return "tcp-badge-info";
}

function prettyMetric(value: any) {
  const num = Number(value);
  if (!Number.isFinite(num)) return "n/a";
  return num.toFixed(3);
}

export function OptimizationCampaignsPanel({
  client,
  toast,
}: {
  client: any;
  toast: (tone: string, message: string) => void;
}) {
  const queryClient = useQueryClient();
  const [selectedCampaignId, setSelectedCampaignId] = useState<string>("");
  const [form, setForm] = useState({
    name: "",
    sourceWorkflowId: "",
    objectiveRef: "objective.md",
    evalRef: "eval.yaml",
    mutationPolicyRef: "mutation_policy.yaml",
    scopeRef: "scope.yaml",
    budgetRef: "budget.yaml",
    startImmediately: true,
  });

  const workflowsQuery = useQuery({
    queryKey: ["optimizations", "workflows", "automations-v2"],
    queryFn: () =>
      client?.automationsV2?.list?.().catch(() => ({ automations: [] })) ??
      Promise.resolve({ automations: [] }),
  });

  const campaignsQuery = useQuery({
    queryKey: ["optimizations", "list"],
    queryFn: () =>
      client?.optimizations?.list?.().catch(() => ({ optimizations: [] })) ??
      Promise.resolve({ optimizations: [] }),
  });

  const campaigns = useMemo(
    () => toArray(campaignsQuery.data, "optimizations"),
    [campaignsQuery.data]
  );

  const selectedId = selectedCampaignId || String(campaigns[0]?.optimization_id || "").trim();

  const detailQuery = useQuery({
    queryKey: ["optimizations", "detail", selectedId],
    enabled: !!selectedId,
    queryFn: () => client.optimizations.get(selectedId),
  });

  const experimentsQuery = useQuery({
    queryKey: ["optimizations", "experiments", selectedId],
    enabled: !!selectedId,
    queryFn: () => client.optimizations.listExperiments(selectedId),
  });

  const createMutation = useMutation({
    mutationFn: async () => {
      const sourceWorkflowId = String(form.sourceWorkflowId || "").trim();
      if (!sourceWorkflowId) throw new Error("Source workflow is required.");
      const payload = {
        name: String(form.name || "").trim() || undefined,
        source_workflow_id: sourceWorkflowId,
        artifacts: {
          objective_ref: String(form.objectiveRef || "").trim(),
          eval_ref: String(form.evalRef || "").trim(),
          mutation_policy_ref: String(form.mutationPolicyRef || "").trim(),
          scope_ref: String(form.scopeRef || "").trim(),
          budget_ref: String(form.budgetRef || "").trim(),
        },
      };
      const created = await client.optimizations.create(payload);
      if (form.startImmediately) {
        const optimizationId = String(created?.optimization?.optimization_id || "").trim();
        if (optimizationId) {
          await client.optimizations.action(optimizationId, { action: "start" });
        }
      }
      return created;
    },
    onSuccess: async (payload: any) => {
      const optimizationId = String(payload?.optimization?.optimization_id || "").trim();
      toast("ok", "Optimization campaign created.");
      if (optimizationId) setSelectedCampaignId(optimizationId);
      await queryClient.invalidateQueries({ queryKey: ["optimizations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const actionMutation = useMutation({
    mutationFn: async ({
      optimizationId,
      action,
      experimentId,
    }: {
      optimizationId: string;
      action: string;
      experimentId?: string;
    }) => client.optimizations.action(optimizationId, { action, experiment_id: experimentId }),
    onSuccess: async () => {
      toast("ok", "Optimization action applied.");
      await queryClient.invalidateQueries({ queryKey: ["optimizations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const applyMutation = useMutation({
    mutationFn: async ({
      optimizationId,
      experimentId,
    }: {
      optimizationId: string;
      experimentId: string;
    }) => client.optimizations.applyWinner(optimizationId, experimentId),
    onSuccess: async () => {
      toast("ok", "Approved winner applied to the live workflow.");
      await queryClient.invalidateQueries({ queryKey: ["optimizations"] });
      await queryClient.invalidateQueries({ queryKey: ["optimizations", "detail", selectedId] });
      await queryClient.invalidateQueries({
        queryKey: ["optimizations", "experiments", selectedId],
      });
      await queryClient.invalidateQueries({
        queryKey: ["optimizations", "workflows", "automations-v2"],
      });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const workflows = toArray(workflowsQuery.data, "automations").map((row: any) => ({
    id: String(row?.automation_id || row?.automationId || row?.id || "").trim(),
    name: String(row?.name || row?.automation_id || "Workflow").trim(),
  }));
  const detail = detailQuery.data?.optimization || null;
  const experiments = toArray(experimentsQuery.data, "experiments");
  const baseline = detail?.baseline_metrics || null;

  return (
    <div className="grid gap-4 lg:grid-cols-[minmax(320px,380px),minmax(0,1fr)]">
      <div className="grid gap-4">
        <div className="rounded-xl border border-slate-700/50 bg-slate-950/40 p-4">
          <div className="mb-1 text-sm font-semibold text-slate-100">New Optimization Campaign</div>
          <div className="tcp-subtle text-xs">
            Create a shadow-eval workflow optimization campaign from an existing automation.
          </div>
          <div className="mt-1 text-xs text-slate-500">
            Inspired by{" "}
            <a
              className="text-amber-300 hover:text-amber-200"
              href="https://github.com/karpathy/autoresearch"
              target="_blank"
              rel="noreferrer"
            >
              karpathy/autoresearch
            </a>
            .
          </div>
          <div className="mt-3 grid gap-3">
            <label className="grid gap-1 text-xs text-slate-300">
              <span>Name</span>
              <input
                className="rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 text-sm"
                value={form.name}
                onChange={(e) => setForm((prev) => ({ ...prev, name: e.target.value }))}
                placeholder="Optimize research brief workflow"
              />
            </label>
            <label className="grid gap-1 text-xs text-slate-300">
              <span>Source workflow</span>
              <select
                className="rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 text-sm"
                value={form.sourceWorkflowId}
                onChange={(e) => setForm((prev) => ({ ...prev, sourceWorkflowId: e.target.value }))}
              >
                <option value="">Select a workflow</option>
                {workflows.map((workflow: any) => (
                  <option key={workflow.id} value={workflow.id}>
                    {workflow.name}
                  </option>
                ))}
              </select>
            </label>
            {[
              ["Objective ref", "objectiveRef"],
              ["Eval ref", "evalRef"],
              ["Mutation policy ref", "mutationPolicyRef"],
              ["Scope ref", "scopeRef"],
              ["Budget ref", "budgetRef"],
            ].map(([label, key]) => (
              <label key={key} className="grid gap-1 text-xs text-slate-300">
                <span>{label}</span>
                <input
                  className="rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 text-sm"
                  value={(form as any)[key]}
                  onChange={(e) => setForm((prev) => ({ ...prev, [key]: e.target.value }))}
                />
              </label>
            ))}
            <label className="flex items-center gap-2 text-xs text-slate-300">
              <input
                type="checkbox"
                checked={form.startImmediately}
                onChange={(e) =>
                  setForm((prev) => ({ ...prev, startImmediately: e.target.checked }))
                }
              />
              Start immediately after create
            </label>
            <button
              className="tcp-btn h-10 px-3 text-sm"
              onClick={() => createMutation.mutate()}
              disabled={createMutation.isPending}
            >
              {createMutation.isPending ? "Creating..." : "Create Campaign"}
            </button>
          </div>
        </div>

        <div className="rounded-xl border border-slate-700/50 bg-slate-950/40 p-4">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="text-sm font-semibold text-slate-100">Campaigns</div>
            <span className="tcp-badge-info">{campaigns.length}</span>
          </div>
          <div className="grid gap-2">
            {campaigns.length ? (
              campaigns.map((campaign: any) => {
                const id = String(campaign?.optimization_id || "").trim();
                return (
                  <button
                    key={id}
                    type="button"
                    className={`rounded-xl border px-3 py-3 text-left transition ${
                      selectedId === id
                        ? "border-amber-400/60 bg-amber-400/10"
                        : "border-slate-700/50 bg-slate-950/40 hover:border-slate-600"
                    }`}
                    onClick={() => setSelectedCampaignId(id)}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <div className="font-medium text-slate-100">
                        {String(campaign?.name || id || "Optimization")}
                      </div>
                      <span className={statusTone(campaign?.status)}>
                        {String(campaign?.status || "draft")}
                      </span>
                    </div>
                    <div className="mt-1 text-xs text-slate-400">
                      workflow:{" "}
                      {String(
                        campaign?.source_workflow_name || campaign?.source_workflow_id || "unknown"
                      )}
                    </div>
                  </button>
                );
              })
            ) : (
              <EmptyState text="No optimization campaigns yet." />
            )}
          </div>
        </div>
      </div>

      <div className="grid gap-4">
        {detail ? (
          <>
            <div className="rounded-xl border border-slate-700/50 bg-slate-950/40 p-4">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <div className="text-lg font-semibold text-slate-100">
                    {String(detail?.name || detail?.optimization_id || "Optimization")}
                  </div>
                  <div className="mt-1 text-sm text-slate-400">
                    Source workflow:{" "}
                    {String(
                      detail?.source_workflow_name || detail?.source_workflow_id || "unknown"
                    )}
                  </div>
                  <div className="mt-1 text-xs text-slate-500">
                    baseline: {String(detail?.baseline_snapshot_hash || "").slice(0, 12)}
                  </div>
                </div>
                <div className="flex flex-wrap gap-2">
                  <button
                    className="tcp-btn h-8 px-3 text-xs"
                    onClick={() =>
                      actionMutation.mutate({ optimizationId: selectedId, action: "start" })
                    }
                    disabled={actionMutation.isPending}
                  >
                    Start
                  </button>
                  <button
                    className="tcp-btn h-8 px-3 text-xs"
                    onClick={() =>
                      actionMutation.mutate({ optimizationId: selectedId, action: "pause" })
                    }
                    disabled={actionMutation.isPending}
                  >
                    Pause
                  </button>
                  <button
                    className="tcp-btn h-8 px-3 text-xs"
                    onClick={() =>
                      actionMutation.mutate({ optimizationId: selectedId, action: "resume" })
                    }
                    disabled={actionMutation.isPending}
                  >
                    Resume
                  </button>
                </div>
              </div>

              <div className="mt-4 grid gap-3 md:grid-cols-4">
                <div className="rounded-lg border border-slate-800 bg-slate-950/60 p-3">
                  <div className="text-[11px] uppercase tracking-[0.22em] text-slate-500">
                    Status
                  </div>
                  <div className="mt-1 text-sm text-slate-100">
                    {String(detail?.status || "draft")}
                  </div>
                </div>
                <div className="rounded-lg border border-slate-800 bg-slate-950/60 p-3">
                  <div className="text-[11px] uppercase tracking-[0.22em] text-slate-500">
                    Pass Rate
                  </div>
                  <div className="mt-1 text-sm text-slate-100">
                    {baseline ? prettyMetric(baseline.artifact_validator_pass_rate) : "n/a"}
                  </div>
                </div>
                <div className="rounded-lg border border-slate-800 bg-slate-950/60 p-3">
                  <div className="text-[11px] uppercase tracking-[0.22em] text-slate-500">
                    Unmet Reqs
                  </div>
                  <div className="mt-1 text-sm text-slate-100">
                    {baseline ? prettyMetric(baseline.unmet_requirement_count) : "n/a"}
                  </div>
                </div>
                <div className="rounded-lg border border-slate-800 bg-slate-950/60 p-3">
                  <div className="text-[11px] uppercase tracking-[0.22em] text-slate-500">
                    Blocked Rate
                  </div>
                  <div className="mt-1 text-sm text-slate-100">
                    {baseline ? prettyMetric(baseline.blocked_node_rate) : "n/a"}
                  </div>
                </div>
              </div>

              {detail?.last_pause_reason ? (
                <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-200">
                  {String(detail.last_pause_reason)}
                </div>
              ) : null}
            </div>

            <div className="rounded-xl border border-slate-700/50 bg-slate-950/40 p-4">
              <div className="mb-2 flex items-center justify-between gap-2">
                <div className="text-sm font-semibold text-slate-100">Experiments</div>
                <span className="tcp-badge-info">{experiments.length}</span>
              </div>
              {experiments.length ? (
                <div className="overflow-x-auto">
                  <table className="min-w-full text-left text-sm">
                    <thead className="text-xs uppercase tracking-[0.22em] text-slate-500">
                      <tr>
                        <th className="px-2 py-2">Experiment</th>
                        <th className="px-2 py-2">Status</th>
                        <th className="px-2 py-2">Mutation</th>
                        <th className="px-2 py-2">Pass</th>
                        <th className="px-2 py-2">Recommendation</th>
                        <th className="px-2 py-2">Action</th>
                      </tr>
                    </thead>
                    <tbody>
                      {experiments.map((experiment: any) => {
                        const experimentId = String(experiment?.experiment_id || "").trim();
                        const metrics = experiment?.phase1_metrics || {};
                        return (
                          <tr key={experimentId} className="border-t border-slate-800">
                            <td className="px-2 py-3 text-slate-200">
                              {experimentId || "unknown"}
                            </td>
                            <td className="px-2 py-3">
                              <span className={statusTone(experiment?.status)}>
                                {String(experiment?.status || "draft")}
                              </span>
                            </td>
                            <td className="px-2 py-3 text-slate-300">
                              {String(experiment?.mutation_summary || "pending")}
                            </td>
                            <td className="px-2 py-3 text-slate-300">
                              {metrics ? prettyMetric(metrics.artifact_validator_pass_rate) : "n/a"}
                            </td>
                            <td className="px-2 py-3 text-slate-300">
                              {String(experiment?.promotion_recommendation || "n/a")}
                            </td>
                            <td className="px-2 py-3">
                              <div className="flex flex-wrap gap-2">
                                <button
                                  className="tcp-btn h-7 px-2 text-xs"
                                  onClick={() =>
                                    actionMutation.mutate({
                                      optimizationId: selectedId,
                                      action: "approve_winner",
                                      experimentId,
                                    })
                                  }
                                  disabled={!experimentId || actionMutation.isPending}
                                >
                                  Approve
                                </button>
                                <button
                                  className="tcp-btn h-7 px-2 text-xs"
                                  onClick={() =>
                                    actionMutation.mutate({
                                      optimizationId: selectedId,
                                      action: "reject_winner",
                                      experimentId,
                                    })
                                  }
                                  disabled={!experimentId || actionMutation.isPending}
                                >
                                  Reject
                                </button>
                                <button
                                  className="tcp-btn h-7 px-2 text-xs"
                                  onClick={() =>
                                    applyMutation.mutate({
                                      optimizationId: selectedId,
                                      experimentId,
                                    })
                                  }
                                  disabled={
                                    !experimentId ||
                                    applyMutation.isPending ||
                                    String(experiment?.status || "")
                                      .trim()
                                      .toLowerCase() !== "promotion_approved"
                                  }
                                >
                                  Apply
                                </button>
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              ) : (
                <EmptyState text="No experiments recorded for this campaign yet." />
              )}
            </div>
          </>
        ) : (
          <div className="rounded-xl border border-slate-700/50 bg-slate-950/40 p-6">
            <EmptyState text="Select or create an optimization campaign to inspect it." />
          </div>
        )}
      </div>
    </div>
  );
}
