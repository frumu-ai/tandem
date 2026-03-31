import { AnimatePresence, motion } from "motion/react";

type ActiveTab = "create" | "calendar" | "list" | "running" | "optimize" | "approvals";
type CreateMode = "simple" | "advanced";

type AutomationsPageTabsProps = {
  tab: ActiveTab;
  setTab: (tab: ActiveTab) => void;
  createMode: CreateMode;
  setCreateMode: (mode: CreateMode) => void;
  selectedRunId: string;
  setSelectedRunId: (runId: string) => void;
  advancedEditAutomation: any | null;
  setAdvancedEditAutomation: (automation: any | null) => void;
  client: any;
  api: any;
  toast: any;
  navigate: any;
  providerStatus: { defaultProvider: string; defaultModel: string };
  PageCardComponent: any;
  CreateWizardComponent: any;
  MyAutomationsComponent: any;
  AdvancedMissionBuilderPanelComponent: any;
  OptimizationCampaignsPanelComponent: any;
  SpawnApprovalsComponent: any;
};

export function AutomationsPageTabs({
  tab,
  setTab,
  createMode,
  setCreateMode,
  selectedRunId,
  setSelectedRunId,
  advancedEditAutomation,
  setAdvancedEditAutomation,
  client,
  api,
  toast,
  navigate,
  providerStatus,
  PageCardComponent,
  CreateWizardComponent,
  MyAutomationsComponent,
  AdvancedMissionBuilderPanelComponent,
  OptimizationCampaignsPanelComponent,
  SpawnApprovalsComponent,
}: AutomationsPageTabsProps) {
  const tabs: { id: ActiveTab; label: string; icon: string }[] = [
    { id: "create", label: "Create", icon: "sparkles" },
    { id: "calendar", label: "Calendar", icon: "calendar" },
    { id: "list", label: "List", icon: "clipboard-list" },
    { id: "running", label: "Tasks", icon: "activity" },
    { id: "optimize", label: "Optimize", icon: "flask-conical" },
    { id: "approvals", label: "Active Teams", icon: "users" },
  ];

  return (
    <div className="grid gap-4">
      <div className="flex gap-1 rounded-xl border border-slate-700/50 bg-slate-900/40 p-1">
        {tabs.map((entry) => (
          <button
            key={entry.id}
            onClick={() => setTab(entry.id)}
            className={`flex flex-1 items-center justify-center gap-1.5 rounded-lg px-3 py-2 text-sm font-medium transition-all ${
              tab === entry.id
                ? "bg-amber-500/20 text-amber-300 shadow-sm"
                : "text-slate-400 hover:text-slate-200"
            }`}
          >
            <i data-lucide={entry.icon}></i>
            <span>{entry.label}</span>
          </button>
        ))}
      </div>

      <AnimatePresence mode="wait">
        <motion.div
          key={tab}
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -6 }}
          transition={{ duration: 0.15 }}
        >
          {tab === "create" ? (
            <PageCardComponent
              title="Create an Automation"
              subtitle="Describe what you want, pick a schedule, and Tandem handles the rest"
            >
              <div className="grid gap-4">
                <div className="rounded-xl border border-slate-700/50 bg-slate-950/50 p-4">
                  <div className="mb-2 text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
                    Builder Mode
                  </div>
                  <div className="tcp-subtle text-xs">
                    Keep the simple wizard for quick automations, or switch to the advanced swarm
                    builder for orchestrated mission planning.
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      type="button"
                      className={`tcp-btn h-9 px-3 text-sm ${
                        createMode === "simple"
                          ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                          : ""
                      }`}
                      onClick={() => {
                        setCreateMode("simple");
                        setAdvancedEditAutomation(null);
                      }}
                    >
                      Simple Wizard
                    </button>
                    <button
                      type="button"
                      className={`tcp-btn h-9 px-3 text-sm ${
                        createMode === "advanced"
                          ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                          : ""
                      }`}
                      onClick={() => setCreateMode("advanced")}
                    >
                      Advanced Swarm Builder
                    </button>
                  </div>
                </div>

                {createMode === "advanced" ? (
                  <AdvancedMissionBuilderPanelComponent
                    client={client}
                    api={api}
                    toast={toast}
                    defaultProvider={providerStatus.defaultProvider}
                    defaultModel={providerStatus.defaultModel}
                    editingAutomation={advancedEditAutomation}
                    onShowAutomations={() => {
                      setAdvancedEditAutomation(null);
                      setTab("calendar");
                    }}
                    onShowRuns={() => {
                      setAdvancedEditAutomation(null);
                      setTab("running");
                    }}
                    onClearEditing={() => setAdvancedEditAutomation(null)}
                  />
                ) : (
                  <CreateWizardComponent
                    client={client}
                    api={api}
                    toast={toast}
                    navigate={navigate}
                    defaultProvider={providerStatus.defaultProvider}
                    defaultModel={providerStatus.defaultModel}
                  />
                )}
              </div>
            </PageCardComponent>
          ) : tab === "calendar" ? (
            <PageCardComponent
              title="Automation Calendar"
              subtitle="Weekly schedule view for cron automations"
            >
              <MyAutomationsComponent
                client={client}
                toast={toast}
                navigate={navigate}
                viewMode="calendar"
                selectedRunId={selectedRunId}
                onSelectRunId={setSelectedRunId}
                onOpenRunningView={() => setTab("running")}
                onOpenAdvancedEdit={(automation: any) => {
                  setAdvancedEditAutomation(automation);
                  setCreateMode("advanced");
                  setTab("create");
                }}
              />
            </PageCardComponent>
          ) : tab === "list" ? (
            <PageCardComponent
              title="My Automations"
              subtitle="Installed packs, routines and run history"
            >
              <MyAutomationsComponent
                client={client}
                toast={toast}
                navigate={navigate}
                viewMode="list"
                selectedRunId={selectedRunId}
                onSelectRunId={setSelectedRunId}
                onOpenRunningView={() => setTab("running")}
                onOpenAdvancedEdit={(automation: any) => {
                  setAdvancedEditAutomation(automation);
                  setCreateMode("advanced");
                  setTab("create");
                }}
              />
            </PageCardComponent>
          ) : tab === "running" ? (
            <PageCardComponent
              title="Live Running Tasks"
              subtitle="Inspect active runs and open detailed event logs for each run"
            >
              <MyAutomationsComponent
                client={client}
                toast={toast}
                navigate={navigate}
                viewMode="running"
                selectedRunId={selectedRunId}
                onSelectRunId={setSelectedRunId}
                onOpenRunningView={() => setTab("running")}
                onOpenAdvancedEdit={(automation: any) => {
                  setAdvancedEditAutomation(automation);
                  setCreateMode("advanced");
                  setTab("create");
                }}
              />
            </PageCardComponent>
          ) : tab === "optimize" ? (
            <PageCardComponent
              title="Workflow Optimization"
              subtitle="Create and inspect overnight shadow-eval optimization campaigns"
            >
              <OptimizationCampaignsPanelComponent client={client} toast={toast} />
            </PageCardComponent>
          ) : (
            <PageCardComponent
              title="Active Teams"
              subtitle="Running team instances and pending spawn approvals"
            >
              <SpawnApprovalsComponent client={client} toast={toast} />
            </PageCardComponent>
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
