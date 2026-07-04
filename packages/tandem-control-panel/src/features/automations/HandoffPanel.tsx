import { useQuery } from "@tanstack/react-query";
import { motion, AnimatePresence } from "motion/react";
import { useState } from "react";
import { Icon } from "../../ui/Icon";

function formatTs(ms: number) {
  if (!ms) return "—";
  return new Date(ms).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function HandoffRow({
  handoff,
  bucket,
}: {
  handoff: any;
  bucket: "inbox" | "approved" | "archived";
}) {
  const [open, setOpen] = useState(false);
  const bucketColor =
    bucket === "inbox"
      ? "text-amber-400 border-amber-400/30 bg-amber-400/10"
      : bucket === "approved"
        ? "text-emerald-400 border-emerald-400/30 bg-emerald-400/10"
        : "text-slate-400 border-slate-600/40 bg-slate-800/40";

  return (
    <div className={`rounded-lg border p-3 ${bucketColor} transition-colors`}>
      <button
        className="flex w-full items-start justify-between gap-3 text-left"
        onClick={() => setOpen((v) => !v)}
      >
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <code className="rounded bg-slate-950/50 px-1.5 py-0.5 font-mono text-xs text-slate-200">
              {handoff.handoff_id}
            </code>
            {handoff.artifact_type && (
              <span className="rounded border border-current/40 bg-current/10 px-1.5 py-0.5 text-xs font-medium">
                {handoff.artifact_type}
              </span>
            )}
          </div>
          <div className="mt-1 text-xs text-slate-400">
            From <span className="font-mono text-slate-300">{handoff.source_automation_id}</span> ·{" "}
            {formatTs(handoff.created_at_ms)}
            {handoff.consumed_at_ms && (
              <span className="ml-2 text-slate-500">
                consumed {formatTs(handoff.consumed_at_ms)}
              </span>
            )}
          </div>
        </div>
        <Icon
          name={open ? "chevron-up" : "chevron-down"}
          className="mt-0.5 h-4 w-4 shrink-0 opacity-60"
        />
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="overflow-hidden"
          >
            <div className="mt-3 rounded-lg border border-slate-700/40 bg-slate-950/60 p-3 font-mono text-xs text-slate-300">
              <pre className="max-h-60 overflow-auto whitespace-pre-wrap break-all">
                {JSON.stringify(handoff, null, 2)}
              </pre>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

const TABS = ["inbox", "approved", "archived"] as const;
type Tab = (typeof TABS)[number];

const TAB_LABELS: Record<Tab, string> = {
  inbox: "Inbox",
  approved: "Approved",
  archived: "Archived",
};

const TAB_COLORS: Record<Tab, string> = {
  inbox: "text-amber-400 border-amber-400",
  approved: "text-emerald-400 border-emerald-400",
  archived: "text-slate-400 border-slate-500",
};

export function HandoffPanel({ automationId, client }: { automationId: string; client: any }) {
  const [tab, setTab] = useState<Tab>("approved");

  const handoffsQuery = useQuery({
    queryKey: ["automations", "v2", automationId, "handoffs"],
    enabled: !!automationId && !!client?.automationsV2?.listHandoffs,
    queryFn: () => client.automationsV2.listHandoffs(automationId),
    refetchInterval: 15_000,
  });

  const data = handoffsQuery.data as any;
  const counts = data?.counts ?? { inbox: 0, approved: 0, archived: 0, total: 0 };
  const items: any[] = data?.[tab] ?? [];

  return (
    <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="text-xs uppercase tracking-wide text-slate-500">Handoff Inbox</div>
        {handoffsQuery.isLoading && <span className="text-xs text-slate-500">Loading…</span>}
        {handoffsQuery.isError && (
          <span className="text-xs text-red-400">Failed to load handoffs</span>
        )}
        {data && (
          <span className="rounded-full bg-slate-800/60 px-2 py-0.5 text-xs text-slate-400">
            {counts.total} total
          </span>
        )}
      </div>

      {/* Tabs */}
      <div className="flex gap-1 border-b border-slate-800/60 pb-0">
        {TABS.map((t) => (
          <button
            key={t}
            className={`border-b-2 px-3 pb-2 pt-1 text-xs font-medium transition-colors ${
              tab === t
                ? `${TAB_COLORS[t]} border-current`
                : "border-transparent text-slate-500 hover:text-slate-300"
            }`}
            onClick={() => setTab(t)}
          >
            {TAB_LABELS[t]}
            {counts[t] > 0 && (
              <span className="ml-1.5 rounded-full bg-current/20 px-1.5 py-0.5 text-xs">
                {counts[t]}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Items */}
      <div className="grid gap-2">
        {handoffsQuery.isPending ? (
          <div className="text-xs text-slate-500">Loading handoffs…</div>
        ) : items.length === 0 ? (
          <div className="rounded-lg border border-dashed border-slate-700/50 p-4 text-center text-xs text-slate-500">
            No {TAB_LABELS[tab].toLowerCase()} handoffs for this automation.
          </div>
        ) : (
          items.map((h: any) => <HandoffRow key={h.handoff_id} handoff={h} bucket={tab} />)
        )}
      </div>

      {data?.handoff_config && (
        <div className="rounded-lg border border-slate-800/50 bg-slate-950/30 p-3">
          <div className="mb-2 text-xs uppercase tracking-wide text-slate-600">
            Directory config
          </div>
          <div className="grid gap-1 font-mono text-xs text-slate-500">
            <div>
              <span className="text-amber-400/70">inbox</span> /{data.handoff_config.inbox_dir}
            </div>
            <div>
              <span className="text-emerald-400/70">approved</span> /
              {data.handoff_config.approved_dir}
            </div>
            <div>
              <span className="text-slate-400/70">archived</span> /
              {data.handoff_config.archived_dir}
            </div>
            <div className="mt-1">
              {data.handoff_config.auto_approve ? (
                <span className="text-emerald-400/70">⚡ auto-approve enabled</span>
              ) : (
                <span className="text-amber-400/70">🔒 manual approval required</span>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
