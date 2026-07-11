import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AnimatedPage, Badge, LoadingState, PanelCard, Toolbar } from "../ui/index.tsx";
import { Icon } from "../ui/Icon";
import type { AppPageProps } from "./pageTypes";
import {
  evidenceCompleteness,
  receiptCorrelation,
  receiptOptionLabel,
  slackIdentityOf,
  slackReceiptRuns,
} from "./slackGovernanceReceiptModel.mjs";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function safeString(value: any, fallback = "") {
  const text = String(value ?? "").trim();
  return text || fallback;
}

function runIdOf(run: any) {
  return safeString(run?.run_id || run?.runID || run?.id);
}

function timeOf(row: any) {
  return Number(row?.updated_at_ms || row?.updatedAtMs || row?.created_at_ms || row?.createdAtMs || 0);
}

function sortedRecent(rows: any[]) {
  return [...rows].sort((a, b) => timeOf(b) - timeOf(a));
}

function listText(values: any, fallback = "None recorded") {
  const rows = Array.isArray(values) ? values.map((value) => safeString(value)).filter(Boolean) : [];
  return rows.length ? rows.join(", ") : fallback;
}

function evidencePackage(payload: any) {
  return payload?.evidence_package || payload?.evidencePackage || payload || {};
}

function toneForDecision(value: any): "ok" | "warn" | "err" | "ghost" | "info" {
  const decision = safeString(value).toLowerCase();
  if (decision.includes("allow") || decision.includes("approved")) return "ok";
  if (decision.includes("approval")) return "warn";
  if (decision.includes("deny") || decision.includes("blocked") || decision.includes("failed")) return "err";
  return "ghost";
}

function Metric({ label, value }: { label: string; value: any }) {
  return (
    <div className="rounded-md border border-white/8 bg-black/20 px-3 py-2">
      <div className="tcp-subtle text-xs">{label}</div>
      <div className="mt-1 truncate text-sm font-semibold text-white">{safeString(value, "0")}</div>
    </div>
  );
}

function ToolList({ title, rows, tone }: { title: string; rows: any[]; tone: "ok" | "warn" | "ghost" }) {
  return (
    <div className="min-w-0 rounded-md border border-white/8 bg-black/20 p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="text-sm font-semibold text-white">{title}</div>
        <Badge tone={tone}>{rows.length}</Badge>
      </div>
      <div className="grid gap-1">
        {rows.length ? (
          rows.slice(0, 14).map((tool) => (
            <code key={safeString(tool)} className="truncate rounded bg-black/30 px-2 py-1 text-xs text-slate-200">
              {safeString(tool)}
            </code>
          ))
        ) : (
          <div className="tcp-subtle text-sm">None recorded</div>
        )}
      </div>
    </div>
  );
}

function SlackIdentityCard({ run, packagePayload }: { run: any; packagePayload: any }) {
  const identity = slackIdentityOf(run);
  const correlation = receiptCorrelation(run, packagePayload);
  if (!identity) return null;
  return (
    <PanelCard
      title="Slack Identity"
      subtitle={
        correlation.identityConsistent
          ? "Run and evidence identities agree"
          : "Run and evidence identities disagree — inspect before trusting this receipt"
      }
    >
      <div className="grid gap-3 md:grid-cols-3">
        <Metric label="Workspace (team)" value={identity.teamId || "unknown"} />
        <Metric label="Channel" value={identity.channelId || "unknown"} />
        <Metric label="Slack user" value={identity.userId || "unknown"} />
        <Metric label="Thread" value={identity.threadTs || "none"} />
        <Metric label="Context run" value={correlation.contextRunId} />
        <Metric
          label="Evidence run"
          value={correlation.packageContextRunId || "not exported yet"}
        />
      </div>
    </PanelCard>
  );
}

export function SlackGovernanceReceiptPage({ api, toast }: AppPageProps) {
  const [selectedContextRunId, setSelectedContextRunId] = useState("");

  const contextRunsQuery = useQuery({
    queryKey: ["slack-governance-receipts", "context-runs"],
    queryFn: () =>
      api("/api/engine/context/runs?limit=120&run_type=session&source=channel%3Aslack").catch(
        (error: any) => ({
          runs: [],
          error: String(error?.message || error),
        })
      ),
    refetchInterval: 10000,
  });

  // Server-side `source=channel:slack` filtering plus a defensive client-side
  // pass: receipts on this page are exclusively Slack-originated session runs,
  // never "whatever context run happens to be newest".
  const contextRuns = sortedRecent(slackReceiptRuns(toArray(contextRunsQuery.data, "runs")));
  const effectiveRunId = safeString(selectedContextRunId || runIdOf(contextRuns[0]));

  const ledgerQuery = useQuery({
    queryKey: ["slack-governance-receipts", "ledger", effectiveRunId],
    enabled: !!effectiveRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(effectiveRunId)}/ledger?tail=200`).catch(
        (error: any) => ({ records: [], summary: {}, tool_manifest: {}, error: String(error?.message || error) })
      ),
    refetchInterval: 10000,
  });

  const evidenceQuery = useQuery({
    queryKey: ["slack-governance-receipts", "evidence", effectiveRunId],
    enabled: !!effectiveRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(effectiveRunId)}/governance-evidence`).catch(
        (error: any) => ({ evidence_package: null, error: String(error?.message || error) })
      ),
    refetchInterval: 15000,
  });

  const ledger = ledgerQuery.data || {};
  const manifest = ledger.tool_manifest || {};
  const packagePayload = evidencePackage(evidenceQuery.data);
  const actors = packagePayload.actors || {};
  const run = packagePayload.run || {};
  const decisions = toArray(packagePayload.policy_decisions, "policy_decisions");
  const approvals = toArray(packagePayload.approvals?.gate_history, "gate_history");
  const memoryAudit = toArray(packagePayload.memory_audit, "memory_audit");
  const protectedEvents = toArray(packagePayload.audit?.protected_events, "protected_events");
  const artifacts = toArray(packagePayload.artifacts, "artifacts");
  const redactions = toArray(packagePayload.redactions, "redactions");
  const limitations = toArray(packagePayload.limitations, "limitations");
  const finalOutcome = packagePayload.final_outcome || {};
  const pendingApproval = packagePayload.approvals?.pending_gate;

  const counts = run.counts || {};
  const receiptUnavailable = !!evidenceQuery.data?.error;
  const evidenceMissing: string[] = receiptUnavailable
    ? []
    : evidenceCompleteness(evidenceQuery.data ? packagePayload : null).missing;
  const selectedRun = useMemo(
    () => contextRuns.find((candidate) => runIdOf(candidate) === effectiveRunId),
    [contextRuns, effectiveRunId]
  );

  return (
    <AnimatedPage className="space-y-4">
      <Toolbar className="justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Icon name="file-check-2" className="h-4 w-4 text-emerald-300" />
            <h1 className="tcp-page-title">Slack Governance Receipts</h1>
          </div>
          <p className="tcp-subtle mt-1 truncate">
            {effectiveRunId ? `Context run ${effectiveRunId}` : "Waiting for a governed Slack run"}
          </p>
        </div>
        <select
          aria-label="Select Slack governance receipt"
          className="tcp-input min-w-[18rem]"
          value={effectiveRunId}
          onChange={(event) => setSelectedContextRunId(event.currentTarget.value)}
        >
          {contextRuns.map((run) => (
            <option key={runIdOf(run)} value={runIdOf(run)}>
              {receiptOptionLabel(run)}
            </option>
          ))}
        </select>
      </Toolbar>

      <h2 className="sr-only">Governance receipt evidence</h2>

      {!effectiveRunId && contextRunsQuery.isLoading ? (
        <LoadingState title="Loading receipts" text="Fetching governed Slack runs." />
      ) : !effectiveRunId ? (
        <PanelCard title="No receipts" subtitle="No governed Slack runs have been recorded yet.">
          <div className="tcp-subtle">
            Receipts appear after a signed Slack message runs through the governed ingress. The
            five-profile ACME E2E (`acme_slack_demo` in tandem-server) exercises and persists this
            exact flow.
          </div>
        </PanelCard>
      ) : (
        <>
          <div className="grid gap-3 md:grid-cols-4">
            <Metric label="Tool calls" value={counts.tool_calls ?? ledger.summary?.record_count ?? 0} />
            <Metric label="Policy decisions" value={counts.policy_decisions ?? decisions.length} />
            <Metric label="Approvals" value={counts.approval_records ?? approvals.length} />
            <Metric label="Memory audit" value={counts.memory_audit_records ?? memoryAudit.length} />
          </div>

          <SlackIdentityCard run={selectedRun} packagePayload={packagePayload} />

          {evidenceMissing.length && !receiptUnavailable ? (
            <PanelCard
              title="Partial evidence"
              subtitle="Sections missing from the persisted evidence package — not zeros."
            >
              <div className="flex flex-wrap gap-2">
                {evidenceMissing.map((section: string) => (
                  <Badge key={section} tone="warn">
                    {section}
                  </Badge>
                ))}
              </div>
            </PanelCard>
          ) : null}

          {receiptUnavailable ? (
            <PanelCard
              title="Governance export unavailable"
              subtitle="The live ledger is still shown below; full receipt export may require premium governance."
              actions={
                <button
                  type="button"
                  aria-label="Show export error"
                  className="tcp-btn-secondary"
                  onClick={() => toast("warn", safeString(evidenceQuery.data?.error, "Export unavailable"))}
                >
                  <Icon name="info" className="h-4 w-4" />
                </button>
              }
            >
              <div className="tcp-subtle text-sm">{safeString(evidenceQuery.data?.error)}</div>
            </PanelCard>
          ) : null}

          <PanelCard title="Requester" subtitle={safeString(run.goal || selectedRun?.objective, "No goal captured")}>
            <div className="grid gap-3 md:grid-cols-4">
              <Metric label="Tenant actor" value={actors.tenant_actor_id || run.tenant_context?.actor_id || "unknown"} />
              <Metric label="Department" value={listText(actors.requester_org_units)} />
              <Metric label="Roles" value={listText(actors.requester_roles)} />
              <Metric label="Grant IDs" value={listText(actors.requester_grant_ids)} />
            </div>
          </PanelCard>

          <PanelCard
            title="Final Slack Outcome"
            subtitle="Persisted response and execution state for this governed Slack run"
          >
            <div className="grid gap-3 md:grid-cols-[2fr_1fr]">
              <div className="min-w-0 rounded-md border border-white/8 bg-black/20 p-3">
                <div className="text-sm font-semibold text-white">Slack-visible response</div>
                <div className="tcp-subtle mt-1 whitespace-pre-wrap break-words text-sm">
                  {safeString(finalOutcome.slack_visible_response, "No Slack response captured")}
                </div>
              </div>
              <div className="grid gap-2">
                <Metric label="Context status" value={finalOutcome.context_status || selectedRun?.status || "unknown"} />
                <Metric label="Automation status" value={finalOutcome.automation_status || "not applicable"} />
                <Metric
                  label="Approval state"
                  value={pendingApproval ? "Approval required" : approvals.length ? "Decided" : "Not required"}
                />
              </div>
            </div>
          </PanelCard>

          <PanelCard
            title="Tools"
            subtitle={
              manifest.used_subset_offered === false
                ? `Unoffered use detected: ${listText(manifest.used_unoffered)}`
                : "Used tools remain within the offered set"
            }
          >
            <div className="grid gap-3 lg:grid-cols-3">
              <ToolList title="Offered" rows={toArray(manifest.offered, "offered")} tone="ok" />
              <ToolList title="Used" rows={toArray(manifest.used, "used")} tone="ghost" />
              <ToolList title="Hidden by scope" rows={toArray(manifest.hidden_by_scope, "hidden_by_scope")} tone="warn" />
            </div>
          </PanelCard>

          <PanelCard title="Policy Decisions" subtitle={`${decisions.length} decision(s) linked to this run`}>
            <div className="grid gap-2">
              {decisions.length ? (
                decisions.slice(0, 8).map((decision: any) => (
                  <div key={safeString(decision.decision_id)} className="grid gap-2 rounded-md border border-white/8 bg-black/20 p-3 md:grid-cols-[1fr_auto]">
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold text-white">
                        {safeString(decision.tool || decision.resource?.resource_id || decision.decision_id)}
                      </div>
                      <div className="tcp-subtle mt-1 truncate text-xs">
                        {safeString(decision.reason_code)} · {safeString(decision.reason)}
                      </div>
                    </div>
                    <Badge tone={toneForDecision(decision.decision)}>{safeString(decision.decision, "unknown")}</Badge>
                  </div>
                ))
              ) : (
                <div className="tcp-subtle text-sm">No policy decisions linked yet.</div>
              )}
            </div>
          </PanelCard>

          <div className="grid gap-4 lg:grid-cols-2">
            <PanelCard title="Memory Evidence" subtitle={`${memoryAudit.length} memory audit row(s)`}>
              <div className="grid gap-2">
                {memoryAudit.length ? (
                  memoryAudit.slice(0, 8).map((row: any) => (
                    <div key={safeString(row.audit_id)} className="rounded-md border border-white/8 bg-black/20 p-3">
                      <div className="text-sm font-semibold text-white">{safeString(row.action, "memory")}</div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {safeString(row.partition_key, "no partition")} · {safeString(row.status, "unknown")}
                      </div>
                    </div>
                  ))
                ) : (
                  <div className="tcp-subtle text-sm">No memory audit rows linked yet.</div>
                )}
              </div>
            </PanelCard>

            <PanelCard title="Approvals And Redactions" subtitle={`${protectedEvents.length} protected audit event(s)`}>
              <div className="grid gap-2">
                <div className="rounded-md border border-white/8 bg-black/20 p-3">
                  <div className="text-sm font-semibold text-white">Approval evidence</div>
                  <div className="tcp-subtle mt-1 text-xs">
                    {pendingApproval
                      ? `Pending ${safeString(pendingApproval.approval_id || pendingApproval.decision_id, "approval")}`
                      : `${approvals.length} completed approval decision(s)`}
                  </div>
                </div>
                <div className="rounded-md border border-white/8 bg-black/20 p-3">
                  <div className="text-sm font-semibold text-white">Redaction policy</div>
                  <div className="tcp-subtle mt-1 text-xs">
                    {Object.entries(packagePayload.redaction_policy || {})
                      .map(([key, value]) => `${key}: ${value}`)
                      .join(" · ") || "No redaction policy recorded"}
                  </div>
                </div>
                <div className="rounded-md border border-white/8 bg-black/20 p-3">
                  <div className="text-sm font-semibold text-white">Redactions</div>
                  <div className="tcp-subtle mt-1 text-xs">
                    {redactions.length
                      ? listText(redactions.map((row: any) => row.reason || row.kind || row.redaction_id))
                      : "No explicit redactions recorded"}
                  </div>
                </div>
                <div className="rounded-md border border-white/8 bg-black/20 p-3">
                  <div className="text-sm font-semibold text-white">Limitations</div>
                  <div className="tcp-subtle mt-1 text-xs">{listText(limitations)}</div>
                </div>
                <div className="rounded-md border border-white/8 bg-black/20 p-3">
                  <div className="text-sm font-semibold text-white">Artifacts</div>
                  <div className="tcp-subtle mt-1 text-xs">
                    {artifacts.length
                      ? listText(artifacts.map((row: any) => row.name || row.artifact_id || row.node_id))
                      : "No receipt artifacts"}
                  </div>
                </div>
              </div>
            </PanelCard>
          </div>
        </>
      )}
    </AnimatedPage>
  );
}
