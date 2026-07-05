import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { AutomationWebhookManager } from "../features/automations/AutomationWebhookManager";
import { AnimatedPage, EmptyState, LoadingState } from "../ui/index.tsx";
import { Icon } from "../ui/Icon";
import type { AppPageProps } from "./pageTypes";

function safeString(value: unknown) {
  return typeof value === "string" ? value.trim() : "";
}

function automationId(row: any) {
  return safeString(row?.automation_id || row?.automationId || row?.id || row?.routine_id);
}

function automationName(row: any) {
  return (
    safeString(row?.name || row?.title || row?.objective) ||
    automationId(row) ||
    "Workflow automation"
  );
}

function automationStatus(row: any) {
  return safeString(row?.status || row?.state || row?.lifecycle_state || row?.lifecycleState);
}

function triggerId(trigger: any) {
  return safeString(trigger?.trigger_id || trigger?.triggerID);
}

function canonicalProvider(value: unknown) {
  const normalized = safeString(value).toLowerCase();
  if (normalized === "linear.app") return "linear";
  if (normalized === "notion.so" || normalized === "notion.com") return "notion";
  if (normalized === "github.com" || normalized === "gh") return "github";
  return normalized;
}

function triggerProvider(trigger: any) {
  return (
    canonicalProvider(
      trigger?.provider_metadata?.canonical_provider ||
        trigger?.providerMetadata?.canonicalProvider ||
        trigger?.provider
    ) || "generic"
  );
}

function triggerEventKind(trigger: any) {
  return safeString(trigger?.provider_event_kind || trigger?.providerEventKind) || "any event";
}

function deliveryCounts(trigger: any) {
  return trigger?.delivery_counts || trigger?.deliveryCounts || {};
}

function countValue(value: unknown) {
  const parsed = Number(value || 0);
  return Number.isFinite(parsed) ? parsed : 0;
}

function callbackUrl(trigger: any) {
  return safeString(
    trigger?.callback_url || trigger?.callbackUrl || trigger?.callback_path || trigger?.callbackPath
  );
}

function triggerReady(trigger: any) {
  if (!trigger?.enabled) return false;
  const provider = triggerProvider(trigger);
  if (provider === "notion") {
    const verification = trigger?.verification_status || trigger?.verificationStatus;
    return safeString(verification?.status) === "active";
  }
  if (provider === "linear") {
    const status =
      trigger?.provider_secret_status ||
      trigger?.providerSecretStatus ||
      trigger?.secret_status ||
      trigger?.secretStatus;
    return status?.provider_owned === true || status?.providerOwned === true
      ? status?.configured === true
      : trigger?.provider_secret_configured === true || trigger?.providerSecretConfigured === true;
  }
  return true;
}

function hashParams() {
  if (typeof window === "undefined") return new URLSearchParams();
  const [, query = ""] = String(window.location.hash || "").split("?");
  return new URLSearchParams(query);
}

function replaceWebhooksHash(automationIdValue: string) {
  if (typeof window === "undefined") return;
  const query = automationIdValue ? `?automation=${encodeURIComponent(automationIdValue)}` : "";
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}${window.location.search}#/webhooks${query}`
  );
}

function openRunHash(runId: string) {
  if (typeof window === "undefined" || !runId) return;
  window.location.hash = `#/runs?run=${encodeURIComponent(runId)}`;
}

type WebhookAutomationRow = {
  automation: any;
  automationId: string;
  automationName: string;
  triggers: any[];
  error: string;
};

async function loadWebhookRows(client: any): Promise<WebhookAutomationRow[]> {
  if (!client?.automationsV2?.list) return [];
  const payload = await client.automationsV2.list();
  const automations = Array.isArray(payload?.automations) ? payload.automations : [];
  const rows = await Promise.all(
    automations.map(async (automation: any) => {
      const id = automationId(automation);
      if (!id || !client?.automationsV2?.listWebhookTriggers) {
        return {
          automation,
          automationId: id,
          automationName: automationName(automation),
          triggers: [],
          error: id ? "" : "Automation id unavailable",
        };
      }
      try {
        const triggerPayload = await client.automationsV2.listWebhookTriggers(id);
        const triggers = Array.isArray(triggerPayload?.triggers) ? triggerPayload.triggers : [];
        return {
          automation,
          automationId: id,
          automationName: automationName(automation),
          triggers,
          error: "",
        };
      } catch (error) {
        return {
          automation,
          automationId: id,
          automationName: automationName(automation),
          triggers: [],
          error: error instanceof Error ? error.message : String(error),
        };
      }
    })
  );
  return rows.filter((row) => row.automationId);
}

function TriggerSummary({ trigger }: { trigger: any }) {
  const counts = deliveryCounts(trigger);
  const total = countValue(counts.total);
  const accepted = countValue(counts.accepted);
  const rejected = countValue(counts.rejected);
  const duplicate = countValue(counts.duplicate);
  return (
    <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-slate-500">
      <span>{total} deliveries</span>
      <span>{accepted} accepted</span>
      <span>{rejected} rejected</span>
      <span>{duplicate} duplicate</span>
    </div>
  );
}

export function WebhooksPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const [selectedAutomationId, setSelectedAutomationId] = useState(() =>
    safeString(hashParams().get("automation"))
  );
  const [providerFilter, setProviderFilter] = useState("all");
  const [query, setQuery] = useState("");

  const rowsQuery = useQuery({
    queryKey: ["webhooks-hub", "automations"],
    enabled: !!client?.automationsV2?.list,
    queryFn: () => loadWebhookRows(client),
    refetchInterval: 30000,
  });

  const rows = rowsQuery.data || [];
  const allTriggers = useMemo(
    () =>
      rows.flatMap((row) =>
        row.triggers.map((trigger) => ({
          automationId: row.automationId,
          automationName: row.automationName,
          trigger,
        }))
      ),
    [rows]
  );

  const providerOptions = useMemo(() => {
    const providers = new Set(
      allTriggers.map((item) => triggerProvider(item.trigger)).filter(Boolean)
    );
    return ["all", ...Array.from(providers).sort()];
  }, [allTriggers]);

  const filteredRows = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return rows
      .map((row) => {
        const triggers = row.triggers.filter((trigger) => {
          const provider = triggerProvider(trigger);
          if (providerFilter !== "all" && provider !== providerFilter) return false;
          if (!needle) return true;
          const haystack = [
            row.automationName,
            row.automationId,
            triggerId(trigger),
            safeString(trigger?.name),
            provider,
            triggerEventKind(trigger),
            callbackUrl(trigger),
          ]
            .join(" ")
            .toLowerCase();
          return haystack.includes(needle);
        });
        const rowMatches =
          !needle ||
          [row.automationName, row.automationId, automationStatus(row.automation)]
            .join(" ")
            .toLowerCase()
            .includes(needle);
        return { ...row, visibleTriggers: triggers, rowMatches };
      })
      .filter((row) => row.rowMatches || row.visibleTriggers.length > 0);
  }, [providerFilter, query, rows]);

  const selectedRow =
    rows.find((row) => row.automationId === selectedAutomationId) ||
    filteredRows[0] ||
    rows[0] ||
    null;

  useEffect(() => {
    if (!rows.length) return;
    if (!selectedRow?.automationId) return;
    if (selectedAutomationId !== selectedRow.automationId) {
      setSelectedAutomationId(selectedRow.automationId);
      replaceWebhooksHash(selectedRow.automationId);
    }
  }, [rows.length, selectedAutomationId, selectedRow?.automationId]);

  const totals = useMemo(() => {
    let accepted = 0;
    let rejected = 0;
    let duplicate = 0;
    for (const item of allTriggers) {
      const counts = deliveryCounts(item.trigger);
      accepted += countValue(counts.accepted);
      rejected += countValue(counts.rejected);
      duplicate += countValue(counts.duplicate);
    }
    return {
      automations: rows.length,
      triggers: allTriggers.length,
      ready: allTriggers.filter((item) => triggerReady(item.trigger)).length,
      accepted,
      rejected,
      duplicate,
    };
  }, [allTriggers, rows.length]);

  const selectAutomation = (automationIdValue: string) => {
    setSelectedAutomationId(automationIdValue);
    replaceWebhooksHash(automationIdValue);
  };

  const openQueuedRun = (runId: string) => {
    openRunHash(runId);
  };

  const invalidateHubInventory = async () => {
    await queryClient.invalidateQueries({ queryKey: ["webhooks-hub", "automations"] });
  };

  return (
    <AnimatedPage className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-4">
      <section className="grid gap-3 rounded-lg border border-slate-800/70 bg-slate-950/25 p-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold text-slate-100">
              <Icon name="webhook" />
              Webhook hub
            </div>
            <div className="mt-1 max-w-3xl text-xs text-slate-500">
              Create provider triggers, copy callback URLs, import provider-owned secrets, and
              inspect delivery readiness across Automation V2 workflows.
            </div>
          </div>
          <button
            type="button"
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => void rowsQuery.refetch()}
            disabled={rowsQuery.isFetching}
          >
            <Icon name="refresh-cw" />
            Refresh
          </button>
        </div>
        <div className="grid gap-2 md:grid-cols-6">
          <div className="rounded-md border border-slate-800/70 bg-black/20 p-2">
            <div className="text-[11px] uppercase tracking-[0.14em] text-slate-500">Workflows</div>
            <div className="mt-1 text-lg font-semibold text-slate-100">{totals.automations}</div>
          </div>
          <div className="rounded-md border border-slate-800/70 bg-black/20 p-2">
            <div className="text-[11px] uppercase tracking-[0.14em] text-slate-500">Triggers</div>
            <div className="mt-1 text-lg font-semibold text-slate-100">{totals.triggers}</div>
          </div>
          <div className="rounded-md border border-slate-800/70 bg-black/20 p-2">
            <div className="text-[11px] uppercase tracking-[0.14em] text-slate-500">Ready</div>
            <div className="mt-1 text-lg font-semibold text-emerald-200">{totals.ready}</div>
          </div>
          <div className="rounded-md border border-slate-800/70 bg-black/20 p-2">
            <div className="text-[11px] uppercase tracking-[0.14em] text-slate-500">Accepted</div>
            <div className="mt-1 text-lg font-semibold text-sky-200">{totals.accepted}</div>
          </div>
          <div className="rounded-md border border-slate-800/70 bg-black/20 p-2">
            <div className="text-[11px] uppercase tracking-[0.14em] text-slate-500">Rejected</div>
            <div className="mt-1 text-lg font-semibold text-amber-200">{totals.rejected}</div>
          </div>
          <div className="rounded-md border border-slate-800/70 bg-black/20 p-2">
            <div className="text-[11px] uppercase tracking-[0.14em] text-slate-500">Duplicate</div>
            <div className="mt-1 text-lg font-semibold text-slate-200">{totals.duplicate}</div>
          </div>
        </div>
      </section>

      <div className="grid min-h-0 gap-4 xl:grid-cols-[360px_1fr]">
        <section className="grid min-h-0 grid-rows-[auto_1fr] gap-3 rounded-lg border border-slate-800/70 bg-slate-950/25 p-3">
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <div>
                <div className="text-sm font-semibold text-slate-100">Trigger index</div>
                <div className="text-xs text-slate-500">Across visible Automation V2 workflows</div>
              </div>
              {rowsQuery.isFetching ? (
                <span className="tcp-badge tcp-badge-info">Syncing</span>
              ) : null}
            </div>
            <input
              className="tcp-input h-8 text-xs"
              value={query}
              onInput={(event) => setQuery((event.target as HTMLInputElement).value)}
              placeholder="Search trigger, provider, workflow, URL"
            />
            <select
              className="tcp-input h-8 text-xs"
              value={providerFilter}
              onChange={(event) => setProviderFilter((event.target as HTMLSelectElement).value)}
            >
              {providerOptions.map((provider) => (
                <option key={provider} value={provider}>
                  {provider === "all" ? "All providers" : provider}
                </option>
              ))}
            </select>
          </div>

          <div className="min-h-0 overflow-auto pr-1">
            {rowsQuery.isLoading ? (
              <LoadingState
                title="Loading webhooks"
                text="Reading Automation V2 trigger inventory"
              />
            ) : filteredRows.length ? (
              <div className="grid gap-2">
                {filteredRows.map((row) => (
                  <div key={row.automationId} className="grid gap-2">
                    <button
                      type="button"
                      className={`tcp-list-item text-left ${row.automationId === selectedRow?.automationId ? "border-amber-400/60 bg-amber-400/10" : ""}`}
                      onClick={() => selectAutomation(row.automationId)}
                    >
                      <div className="flex items-start justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate text-sm font-semibold text-slate-100">
                            {row.automationName}
                          </div>
                          <div className="mt-1 truncate text-xs text-slate-500">
                            {row.automationId}
                          </div>
                        </div>
                        <span className="tcp-badge tcp-badge-info">{row.triggers.length}</span>
                      </div>
                      {row.error ? (
                        <div className="mt-2 text-xs text-red-200">{row.error}</div>
                      ) : null}
                    </button>
                    {row.visibleTriggers.map((trigger) => (
                      <button
                        key={
                          triggerId(trigger) || `${row.automationId}-${triggerProvider(trigger)}`
                        }
                        type="button"
                        className={`ml-3 rounded-md border border-slate-800/70 bg-black/20 p-2 text-left hover:border-slate-700 ${row.automationId === selectedRow?.automationId ? "border-slate-700" : ""}`}
                        onClick={() => selectAutomation(row.automationId)}
                      >
                        <div className="flex items-start justify-between gap-2">
                          <div className="min-w-0">
                            <div className="truncate text-xs font-semibold text-slate-200">
                              {safeString(trigger?.name) || triggerProvider(trigger)}
                            </div>
                            <div className="mt-1 truncate text-[11px] text-slate-500">
                              {triggerProvider(trigger)} · {triggerEventKind(trigger)}
                            </div>
                          </div>
                          <span
                            className={`tcp-badge ${triggerReady(trigger) ? "tcp-badge-ok" : "tcp-badge-warn"}`}
                          >
                            {triggerReady(trigger) ? "Ready" : "Setup"}
                          </span>
                        </div>
                        <TriggerSummary trigger={trigger} />
                      </button>
                    ))}
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState
                title="No webhook triggers"
                text="Select a workflow and create the first trigger from the setup panel."
              />
            )}
          </div>
        </section>

        <section className="min-h-0 overflow-auto">
          {selectedRow ? (
            <div className="grid gap-3">
              <div className="rounded-lg border border-slate-800/70 bg-slate-950/25 p-3">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-sm font-semibold text-slate-100">
                      {selectedRow.automationName}
                    </div>
                    <div className="mt-1 truncate text-xs text-slate-500">
                      {selectedRow.automationId}
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {automationStatus(selectedRow.automation) ? (
                      <span className="tcp-badge tcp-badge-info">
                        {automationStatus(selectedRow.automation)}
                      </span>
                    ) : null}
                    <span className="tcp-badge tcp-badge-ghost">
                      {selectedRow.triggers.length} triggers
                    </span>
                  </div>
                </div>
              </div>
              <AutomationWebhookManager
                client={client}
                automationId={selectedRow.automationId}
                toast={toast}
                onSelectRunId={openQueuedRun}
                onOpenRunningView={() => {}}
                onTriggersChanged={invalidateHubInventory}
              />
            </div>
          ) : rowsQuery.isLoading ? (
            <LoadingState title="Loading webhooks" text="Reading workflow inventory" />
          ) : (
            <EmptyState
              title="No workflows"
              text="Create an Automation V2 workflow before adding webhook triggers."
            />
          )}
        </section>
      </div>
    </AnimatedPage>
  );
}
