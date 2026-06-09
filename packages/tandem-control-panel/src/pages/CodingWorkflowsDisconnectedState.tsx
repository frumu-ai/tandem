import { AnimatedPage, Badge, LoadingState, PanelCard } from "../ui/index.tsx";

type Props = {
  acaReason: string;
  acaStatusText: string;
  configPath?: string;
  engineAvailable: boolean;
  missingFields: string[];
  navigateSettings: () => void;
  refreshAcaConnection: () => void;
};

export function CodingWorkflowsConnectingState() {
  return (
    <AnimatedPage className="grid gap-4">
      <PanelCard>
        <LoadingState title="Connecting to ACA" subtitle="Tandem is checking the ACA runtime before loading Coder." />
      </PanelCard>
    </AnimatedPage>
  );
}

export function CodingWorkflowsDisconnectedState({
  acaReason,
  acaStatusText,
  configPath,
  engineAvailable,
  missingFields,
  navigateSettings,
  refreshAcaConnection,
}: Props) {
  return (
    <AnimatedPage className="grid gap-4">
      <PanelCard className="overflow-hidden">
        <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(320px,0.9fr)] xl:items-start">
          <div className="min-w-0">
            <div className="tcp-page-eyebrow">Coder</div>
            <h1 className="tcp-page-title">Coder dashboard</h1>
            <p className="tcp-subtle mt-2 max-w-3xl">
              ACA integration is required for Coder. Connect the ACA control plane so this workspace
              can load registered projects, task intake, and live run details.
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Badge tone={engineAvailable ? "ok" : "warn"}>
                {engineAvailable ? "Engine healthy" : "Engine unavailable"}
              </Badge>
              <Badge tone="warn">ACA disconnected</Badge>
            </div>
            <div className="mt-4 rounded-2xl border border-yellow-500/20 bg-yellow-500/10 p-4">
              <p className="text-sm text-yellow-200">
                <strong>ACA integration required.</strong> Set `ACA_BASE_URL` and make sure the
                control panel can authenticate to ACA with `ACA_API_TOKEN` or `ACA_API_TOKEN_FILE`.
              </p>
              <div className="mt-3 grid gap-2 text-xs text-yellow-100/90">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge tone={engineAvailable ? "ok" : "warn"}>
                    {engineAvailable ? "Engine healthy" : "Engine unavailable"}
                  </Badge>
                  <Badge tone="warn">{acaReason || "aca_disconnected"}</Badge>
                </div>
                <div>{acaStatusText}</div>
                {configPath ? <div className="break-all">Config: {configPath}</div> : null}
                {missingFields.length ? (
                  <div>Missing install fields: {missingFields.join(", ")}</div>
                ) : null}
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <button type="button" className="tcp-btn" onClick={navigateSettings}>
                  <i data-lucide="settings"></i>
                  Open ACA setup
                </button>
                <button type="button" className="tcp-btn" onClick={refreshAcaConnection}>
                  <i data-lucide="refresh-cw"></i>
                  Retry connection
                </button>
              </div>
            </div>
          </div>
        </div>
      </PanelCard>
    </AnimatedPage>
  );
}
