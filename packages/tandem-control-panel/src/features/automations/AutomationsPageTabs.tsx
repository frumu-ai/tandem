import { useEffect } from "react";
import { renderIcons } from "../../app/icons.js";
import { seedAutomationPlanner } from "../chat/chatPageHelpers";

type ActiveTab = "create" | "calendar" | "list" | "running";
type CreateMode = "simple" | "advanced" | "composer";

function SectionTitle({ icon, label }: { icon: string; label: string }) {
  return (
    <span className="inline-flex items-center gap-2">
      <i data-lucide={icon} className="h-4 w-4 text-amber-300/90"></i>
      <span>{label}</span>
    </span>
  );
}

function recreateWorkflowFromPrompt({
  payload,
  toast,
  setAdvancedEditAutomation,
  setCreateMode,
  setTab,
  composerEnabled,
}: {
  payload: any;
  toast: any;
  setAdvancedEditAutomation: (automation: any | null) => void;
  setCreateMode: (mode: CreateMode) => void;
  setTab: (tab: ActiveTab) => void;
  composerEnabled: boolean;
}) {
  const prompt = String(payload?.prompt || "").trim();
  if (!prompt) {
    toast("err", "No original prompt was found for this workflow.");
    return;
  }
  seedAutomationPlanner({
    prompt,
    plan_source: "workflow_recreate",
  });
  setAdvancedEditAutomation(null);
  setCreateMode(composerEnabled ? "composer" : "simple");
  setTab("create");
  toast(
    "ok",
    composerEnabled
      ? "Opened AI Composer with the recovered workflow prompt."
      : "Opened Create with the recovered workflow prompt."
  );
}

type AutomationsPageTabsProps = {
  tab: ActiveTab;
  setTab: (tab: ActiveTab) => void;
  createMode: CreateMode;
  setCreateMode: (mode: CreateMode) => void;
  selectedRunId: string;
  setSelectedRunId: (runId: string) => void;
  advancedEditAutomation: any | null;
  setAdvancedEditAutomation: (automation: any | null) => void;
  navigationLocked: boolean;
  onNavigationLockChange?: (lock: { title: string; message: string } | null) => void;
  client: any;
  api: any;
  toast: any;
  navigate: any;
  providerStatus: { defaultProvider: string; defaultModel: string };
  composerEnabled: boolean;
  PageCardComponent: any;
  CreateWizardComponent: any;
  AutomationComposerPanelComponent: any;
  MyAutomationsComponent: any;
  AdvancedMissionBuilderPanelComponent: any;
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
  navigationLocked,
  onNavigationLockChange,
  client,
  api,
  toast,
  navigate,
  providerStatus,
  composerEnabled,
  PageCardComponent,
  CreateWizardComponent,
  AutomationComposerPanelComponent,
  MyAutomationsComponent,
  AdvancedMissionBuilderPanelComponent,
}: AutomationsPageTabsProps) {
  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  }, [tab, createMode, composerEnabled]);

  const tabs: { id: ActiveTab; label: string; icon: string }[] = [
    { id: "create", label: "Create", icon: "sparkles" },
    { id: "calendar", label: "Calendar", icon: "calendar" },
    { id: "list", label: "Library", icon: "book-open" },
    { id: "running", label: "Run History", icon: "history" },
  ];

  const selectTab = (nextTab: ActiveTab) => {
    if (nextTab !== "running") {
      setSelectedRunId("");
    }
    setTab(nextTab);
  };
  const onRecreateWorkflowAutomation = (payload: any) =>
    recreateWorkflowFromPrompt({
      payload,
      toast,
      setAdvancedEditAutomation,
      setCreateMode,
      setTab,
      composerEnabled,
    });

  return (
    <div className="flex flex-col h-full gap-4">
      <div className="flex gap-1 rounded-xl border border-slate-700/50 bg-slate-900/40 p-1">
        {tabs.map((entry) => (
          <button
            key={entry.id}
            onClick={() => selectTab(entry.id)}
            disabled={navigationLocked}
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

      <div className="flex-1 flex flex-col min-h-0">
        {tab === "create" ? (
          <PageCardComponent
            title={<SectionTitle icon="sparkles" label="Create an Automation" />}
            subtitle="Describe what you want, pick a schedule, and Tandem handles the rest"
            fullHeight
          >
            <div className="flex flex-col flex-1 min-h-0 h-full gap-4">
              {createMode === "composer" && composerEnabled ? (
                <AutomationComposerPanelComponent
                  client={client}
                  api={api}
                  toast={toast}
                  defaultProvider={providerStatus.defaultProvider}
                  defaultModel={providerStatus.defaultModel}
                  onNavigationLockChange={onNavigationLockChange}
                  onShowAutomations={() => {
                    setAdvancedEditAutomation(null);
                    setTab("list");
                  }}
                  onShowRuns={() => {
                    setAdvancedEditAutomation(null);
                    setTab("running");
                  }}
                />
              ) : (
                <>
                  <div className="rounded-xl border border-slate-700/50 bg-slate-950/50 p-4 shrink-0">
                    <div className="mb-2 text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
                      Builder Mode
                    </div>
                    <div className="tcp-subtle text-xs">
                      Keep the simple wizard for quick automations, or switch to Mission Builder for
                      generated multi-step missions you can tune before launch.
                    </div>
                    <div className="mt-3 flex flex-wrap gap-2">
                      <button
                        type="button"
                        disabled={navigationLocked}
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
                        disabled={navigationLocked}
                        className={`tcp-btn h-9 px-3 text-sm ${
                          createMode === "advanced"
                            ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                            : ""
                        }`}
                        onClick={() => setCreateMode("advanced")}
                      >
                        Mission Builder
                      </button>
                      {composerEnabled ? (
                        <button
                          type="button"
                          disabled={navigationLocked}
                          className={`tcp-btn h-9 px-3 text-sm ${
                            createMode === "composer"
                              ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                              : ""
                          }`}
                          onClick={() => {
                            setCreateMode("composer");
                            setAdvancedEditAutomation(null);
                          }}
                        >
                          AI Composer
                        </button>
                      ) : null}
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
                        setTab("list");
                      }}
                      onShowRuns={() => {
                        setAdvancedEditAutomation(null);
                        setTab("running");
                      }}
                      onClearEditing={() => setAdvancedEditAutomation(null)}
                      onNavigationLockChange={onNavigationLockChange}
                    />
                  ) : (
                    <CreateWizardComponent
                      client={client}
                      api={api}
                      toast={toast}
                      navigate={navigate}
                      defaultProvider={providerStatus.defaultProvider}
                      defaultModel={providerStatus.defaultModel}
                      onNavigationLockChange={onNavigationLockChange}
                      onCreated={() => {
                        setAdvancedEditAutomation(null);
                        setTab("list");
                      }}
                    />
                  )}
                </>
              )}
            </div>
          </PageCardComponent>
        ) : tab === "calendar" ? (
          <PageCardComponent
            title={<SectionTitle icon="calendar" label="Automation Calendar" />}
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
              onRecreateWorkflowAutomation={onRecreateWorkflowAutomation}
              defaultRunningSectionsOpen={{
                active: true,
                issues: true,
                history: true,
              }}
            />
          </PageCardComponent>
        ) : tab === "list" ? (
          <PageCardComponent
            title={<SectionTitle icon="book-open" label="Library" />}
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
              onRecreateWorkflowAutomation={onRecreateWorkflowAutomation}
            />
          </PageCardComponent>
        ) : tab === "running" ? (
          <PageCardComponent
            title={<SectionTitle icon="history" label="Run History" />}
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
              onRecreateWorkflowAutomation={onRecreateWorkflowAutomation}
            />
          </PageCardComponent>
        ) : (
          <PageCardComponent
            title={<SectionTitle icon="book-open" label="Library" />}
            subtitle="Saved automations, routines, and run history"
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
              onRecreateWorkflowAutomation={onRecreateWorkflowAutomation}
            />
          </PageCardComponent>
        )}
      </div>
    </div>
  );
}
