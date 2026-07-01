import { useEffect, useState } from "react";
import { AnimatedPage } from "../ui/index.tsx";
import { StatefulRunsPage } from "../features/runs/StatefulRunsPage";
import { ApprovalWaitsView, RecoveryQueueView, WebhookInboxView } from "../features/runs/StatefulRuntimeQueues";
import { renderIcons } from "../app/icons.js";
import type { AppPageProps } from "./pageTypes";

type RunsSurface = "runs" | "webhooks" | "approvals" | "recovery";

const RUNS_SURFACES: Array<{ id: RunsSurface; label: string; icon: string }> = [
  { id: "runs", label: "Runs", icon: "activity" },
  { id: "webhooks", label: "Webhooks", icon: "webhook" },
  { id: "approvals", label: "Approvals", icon: "shield-check" },
  { id: "recovery", label: "Recovery", icon: "life-buoy" },
];

function replaceRunSelectionHash(runId: string) {
  if (typeof window === "undefined" || !runId) return;
  const hash = `#/runs?run=${encodeURIComponent(runId)}`;
  window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}${hash}`);
}

export function RunsPage({ api, client, navigate, toast }: AppPageProps) {
  const [surface, setSurface] = useState<RunsSurface>("runs");

  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  }, [surface]);

  const openRun = (runId: string) => {
    replaceRunSelectionHash(runId);
    setSurface("runs");
  };

  return (
    <AnimatedPage className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-4">
      <div className="flex min-w-0 flex-wrap gap-2">
        {RUNS_SURFACES.map((item) => (
          <button
            key={item.id}
            type="button"
            className={`tcp-filter-chip ${surface === item.id ? "active" : ""}`}
            onClick={() => setSurface(item.id)}
          >
            <i data-lucide={item.icon}></i>
            {item.label}
          </button>
        ))}
      </div>
      <div className="min-h-0">
        {surface === "runs" ? (
          <StatefulRunsPage api={api} client={client} navigate={navigate} />
        ) : surface === "webhooks" ? (
          <WebhookInboxView api={api} navigate={navigate} toast={toast} onOpenRun={openRun} />
        ) : surface === "approvals" ? (
          <ApprovalWaitsView api={api} navigate={navigate} toast={toast} onOpenRun={openRun} />
        ) : (
          <RecoveryQueueView api={api} navigate={navigate} toast={toast} onOpenRun={openRun} />
        )}
      </div>
    </AnimatedPage>
  );
}
