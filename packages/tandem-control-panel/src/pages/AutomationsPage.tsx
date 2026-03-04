import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

// ─── Types ─────────────────────────────────────────────────────────────────

type ExecutionMode = "single" | "team" | "swarm";
type WizardStep = 1 | 2 | 3 | 4;
type ActiveTab = "create" | "list" | "approvals";

interface SchedulePreset {
  label: string;
  desc: string;
  icon: string;
  cron: string;
  intervalSeconds?: number;
}

interface WizardState {
  goal: string;
  schedulePreset: string;
  cron: string;
  mode: ExecutionMode;
  maxAgents: string;
}

// ─── Constants ──────────────────────────────────────────────────────────────

const SCHEDULE_PRESETS: SchedulePreset[] = [
  {
    label: "Every hour",
    desc: "Good for monitoring & alerts",
    icon: "⏰",
    cron: "",
    intervalSeconds: 3600,
  },
  { label: "Every morning", desc: "Daily digest at 9 AM", icon: "☀️", cron: "0 9 * * *" },
  { label: "Every evening", desc: "End-of-day summary at 6 PM", icon: "🌙", cron: "0 18 * * *" },
  { label: "Daily at midnight", desc: "Nightly data processing", icon: "🔄", cron: "0 0 * * *" },
  { label: "Weekly Monday", desc: "Weekly reports & reviews", icon: "📋", cron: "0 9 * * 1" },
  { label: "Manual only", desc: "Run whenever you want", icon: "🎯", cron: "" },
];

const EXECUTION_MODES: {
  id: ExecutionMode;
  label: string;
  icon: string;
  desc: string;
  bestFor: string;
}[] = [
  {
    id: "single",
    label: "Single Agent",
    icon: "🤖",
    desc: "One focused AI handles the whole task",
    bestFor: "Simple, well-defined tasks",
  },
  {
    id: "team",
    label: "Agent Team",
    icon: "👥",
    desc: "Multiple specialized AIs collaborate with a planner and workers",
    bestFor: "Complex multi-step tasks (recommended)",
  },
  {
    id: "swarm",
    label: "Swarm",
    icon: "🐝",
    desc: "A swarm of AIs work in parallel on sub-tasks",
    bestFor: "Large-scale parallel work",
  },
];

const GOAL_EXAMPLES = [
  "Check my email every morning and send me a summary of what's important",
  "Monitor GitHub issues and post daily standup updates to Slack",
  "Summarize my calendar each Sunday and plan the upcoming week",
  "Watch for price changes on competitor products and alert me",
  "Generate a weekly report from our Notion workspace",
];

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

// ─── Wizard Steps ───────────────────────────────────────────────────────────

function Step1Goal({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">
        Describe what you want the AI to do — in plain English. No technical knowledge needed.
      </p>
      <textarea
        className="tcp-input min-h-[120px] text-base"
        placeholder={`e.g. "${GOAL_EXAMPLES[0]}"`}
        value={value}
        onInput={(e) => onChange((e.target as HTMLTextAreaElement).value)}
        autoFocus
      />
      <div className="grid gap-2">
        <p className="text-xs text-slate-500">Need inspiration? Try one of these:</p>
        <div className="flex flex-wrap gap-2">
          {GOAL_EXAMPLES.slice(1).map((ex) => (
            <button
              key={ex}
              className="tcp-btn truncate text-left text-xs"
              style={{ maxWidth: "280px" }}
              onClick={() => onChange(ex)}
            >
              {ex}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function Step2Schedule({
  selected,
  onSelect,
  customCron,
  onCustomCron,
}: {
  selected: string;
  onSelect: (preset: SchedulePreset) => void;
  customCron: string;
  onCustomCron: (v: string) => void;
}) {
  return (
    <div className="grid gap-3">
      <p className="text-sm text-slate-400">When should this automation run?</p>
      <div className="grid gap-2 sm:grid-cols-2">
        {SCHEDULE_PRESETS.map((preset) => (
          <button
            key={preset.label}
            onClick={() => onSelect(preset)}
            className={`tcp-list-item flex flex-col items-start gap-1 text-left transition-all ${
              selected === preset.label ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
          >
            <div className="flex items-center gap-2 font-medium">
              <span>{preset.icon}</span>
              <span>{preset.label}</span>
            </div>
            <span className="tcp-subtle text-xs">{preset.desc}</span>
            {preset.cron ? (
              <code className="rounded bg-slate-800/60 px-1.5 py-0.5 text-xs text-slate-400">
                {preset.cron}
              </code>
            ) : null}
          </button>
        ))}
      </div>
      <div className="grid gap-1">
        <label className="text-xs text-slate-500">Custom cron expression (advanced)</label>
        <input
          className="tcp-input font-mono text-sm"
          placeholder="e.g. 30 8 * * 1-5  (8:30am weekdays)"
          value={customCron}
          onInput={(e) => onCustomCron((e.target as HTMLInputElement).value)}
        />
      </div>
    </div>
  );
}

function Step3Mode({
  selected,
  onSelect,
  maxAgents,
  onMaxAgents,
}: {
  selected: ExecutionMode;
  onSelect: (mode: ExecutionMode) => void;
  maxAgents: string;
  onMaxAgents: (v: string) => void;
}) {
  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">
        How should the AI handle this task? (You can always change this later.)
      </p>
      <div className="grid gap-3">
        {EXECUTION_MODES.map((m) => (
          <button
            key={m.id}
            onClick={() => onSelect(m.id)}
            className={`tcp-list-item flex items-start gap-4 text-left transition-all ${
              selected === m.id ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
          >
            <span className="mt-0.5 text-2xl">{m.icon}</span>
            <div className="grid gap-1">
              <div className="flex items-center gap-2">
                <span className="font-semibold">{m.label}</span>
                {m.id === "team" ? (
                  <span className="rounded-full bg-amber-500/20 px-2 py-0.5 text-xs text-amber-300">
                    Recommended
                  </span>
                ) : null}
              </div>
              <span className="text-sm text-slate-300">{m.desc}</span>
              <span className="tcp-subtle text-xs">Best for: {m.bestFor}</span>
            </div>
            <div
              className="ml-auto mt-1 h-4 w-4 shrink-0 rounded-full border-2 border-slate-600 transition-all data-[checked]:border-amber-400 data-[checked]:bg-amber-400/30"
              data-checked={selected === m.id ? true : undefined}
            />
          </button>
        ))}
      </div>
      {selected === "swarm" ? (
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Max parallel agents</label>
          <input
            type="number"
            min="2"
            max="16"
            className="tcp-input w-24"
            value={maxAgents}
            onInput={(e) => onMaxAgents((e.target as HTMLInputElement).value)}
          />
        </div>
      ) : null}
    </div>
  );
}

function Step4Review({
  wizard,
  onSubmit,
  isPending,
}: {
  wizard: WizardState;
  onSubmit: () => void;
  isPending: boolean;
}) {
  const schedule = wizard.cron
    ? wizard.cron
    : SCHEDULE_PRESETS.find((p) => p.label === wizard.schedulePreset)?.intervalSeconds
      ? `Every ${SCHEDULE_PRESETS.find((p) => p.label === wizard.schedulePreset)!.intervalSeconds! / 3600}h`
      : wizard.schedulePreset || "Manual";

  const modeInfo = EXECUTION_MODES.find((m) => m.id === wizard.mode);

  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">
        Review your automation before deploying. The AI will create and install a pack
        automatically.
      </p>

      {/* Summary card */}
      <div className="rounded-xl border border-slate-700/60 bg-slate-900/40 p-4 grid gap-3">
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Goal</span>
          <span className="text-sm text-slate-100 italic">"{wizard.goal}"</span>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Schedule</span>
            <span className="text-sm font-medium text-slate-200">
              {modeInfo?.icon} {wizard.schedulePreset || schedule}
            </span>
          </div>
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Execution Mode</span>
            <span className="text-sm font-medium text-slate-200">
              {modeInfo?.icon} {modeInfo?.label}
            </span>
          </div>
        </div>
        {wizard.cron ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Cron</span>
            <code className="rounded bg-slate-800/60 px-2 py-1 text-xs text-slate-300">
              {wizard.cron}
            </code>
          </div>
        ) : null}
      </div>

      <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
        💡 Tandem will build a custom automation pack and schedule a{" "}
        <strong className="text-slate-300">{modeInfo?.label}</strong> that runs{" "}
        <strong className="text-slate-300">{wizard.schedulePreset || schedule}</strong>. You can
        pause, edit or delete it anytime.
      </div>

      <button
        className="tcp-btn-primary"
        disabled={isPending || !wizard.goal.trim()}
        onClick={onSubmit}
      >
        {isPending ? "Creating automation…" : "🚀 Create Automation"}
      </button>
    </div>
  );
}

// ─── Wizard Container ───────────────────────────────────────────────────────

function CreateWizard({ client, toast }: { client: any; toast: any }) {
  const queryClient = useQueryClient();
  const [step, setStep] = useState<WizardStep>(1);
  const [wizard, setWizard] = useState<WizardState>({
    goal: "",
    schedulePreset: "Every morning",
    cron: "",
    mode: "team",
    maxAgents: "4",
  });

  const deployMutation = useMutation({
    mutationFn: async () => {
      if (!wizard.goal.trim()) throw new Error("Please describe your goal first.");
      const preset = SCHEDULE_PRESETS.find((p) => p.label === wizard.schedulePreset);
      const schedule = wizard.cron
        ? { cron: wizard.cron }
        : preset?.intervalSeconds
          ? { interval_seconds: preset.intervalSeconds }
          : preset?.cron
            ? { cron: preset.cron }
            : {};

      // Build the prompt for the pack_builder tool via a chat session
      const sessionId = await client.sessions.create({
        title: `Auto: ${wizard.goal.slice(0, 60)}`,
      });
      const prompt = [
        `Create an automation pack for this goal: "${wizard.goal}"`,
        `Execution mode: ${wizard.mode}${wizard.mode === "swarm" ? ` (max ${wizard.maxAgents} agents)` : ""}`,
        wizard.schedulePreset !== "Manual only"
          ? `Schedule: ${wizard.cron || preset?.cron || `every ${preset?.intervalSeconds}s`}`
          : "Run manually on demand",
        "Please use the pack_builder tool to create and install this automation now.",
      ].join("\n");

      await client.sessions.promptAsync(String(sessionId), prompt);
      return { sessionId: String(sessionId) };
    },
    onSuccess: async () => {
      toast("ok", "🎉 Automation created! Check 'My Automations' to see it running.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
        queryClient.invalidateQueries({ queryKey: ["agents"] }),
      ]);
      // Reset wizard
      setWizard({
        goal: "",
        schedulePreset: "Every morning",
        cron: "",
        mode: "team",
        maxAgents: "4",
      });
      setStep(1);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const canAdvance =
    step === 1
      ? wizard.goal.trim().length > 8
      : step === 2
        ? !!wizard.schedulePreset
        : step === 3
          ? !!wizard.mode
          : true;

  const STEPS = ["What?", "When?", "How?", "Review"];

  return (
    <div className="grid gap-4">
      {/* Progress Bar */}
      <div className="flex items-center gap-2">
        {STEPS.map((label, i) => {
          const num = (i + 1) as WizardStep;
          const active = num === step;
          const done = num < step;
          return (
            <div key={label} className="flex-1">
              <button
                className={`mb-1 flex w-full items-center gap-1.5 rounded-lg px-2 py-1 text-xs font-medium transition-all ${
                  active
                    ? "bg-amber-500/20 text-amber-300"
                    : done
                      ? "text-slate-400"
                      : "text-slate-600"
                }`}
                onClick={() => done && setStep(num)}
              >
                <span
                  className={`flex h-5 w-5 items-center justify-center rounded-full text-xs font-bold ${
                    active
                      ? "bg-amber-500 text-black"
                      : done
                        ? "bg-slate-600 text-white"
                        : "bg-slate-800 text-slate-500"
                  }`}
                >
                  {done ? "✓" : num}
                </span>
                {label}
              </button>
              {/* Progress line */}
              <div className="h-0.5 w-full rounded-full bg-slate-800">
                <div
                  className="h-full rounded-full bg-amber-500 transition-all"
                  style={{ width: done ? "100%" : active ? "50%" : "0%" }}
                />
              </div>
            </div>
          );
        })}
      </div>

      {/* Step content */}
      <AnimatePresence mode="wait">
        <motion.div
          key={step}
          initial={{ opacity: 0, x: 16 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: -16 }}
          transition={{ duration: 0.18 }}
        >
          {step === 1 ? (
            <Step1Goal
              value={wizard.goal}
              onChange={(v) => setWizard((s) => ({ ...s, goal: v }))}
            />
          ) : step === 2 ? (
            <Step2Schedule
              selected={wizard.schedulePreset}
              onSelect={(preset) =>
                setWizard((s) => ({
                  ...s,
                  schedulePreset: preset.label,
                  cron: preset.cron,
                }))
              }
              customCron={wizard.cron}
              onCustomCron={(v) => setWizard((s) => ({ ...s, cron: v, schedulePreset: "" }))}
            />
          ) : step === 3 ? (
            <Step3Mode
              selected={wizard.mode}
              onSelect={(mode) => setWizard((s) => ({ ...s, mode }))}
              maxAgents={wizard.maxAgents}
              onMaxAgents={(v) => setWizard((s) => ({ ...s, maxAgents: v }))}
            />
          ) : (
            <Step4Review
              wizard={wizard}
              onSubmit={() => deployMutation.mutate()}
              isPending={deployMutation.isPending}
            />
          )}
        </motion.div>
      </AnimatePresence>

      {/* Navigation */}
      {step < 4 ? (
        <div className="flex justify-between gap-2">
          <button
            className="tcp-btn"
            disabled={step === 1}
            onClick={() => setStep((s) => (s - 1) as WizardStep)}
          >
            ← Back
          </button>
          <button
            className="tcp-btn-primary"
            disabled={!canAdvance}
            onClick={() => setStep((s) => (s + 1) as WizardStep)}
          >
            Next →
          </button>
        </div>
      ) : null}
    </div>
  );
}

// ─── My Automations (combined routines + packs) ─────────────────────────────

function MyAutomations({ client, toast }: { client: any; toast: any }) {
  const queryClient = useQueryClient();

  const routinesQuery = useQuery({
    queryKey: ["automations", "routines"],
    queryFn: () =>
      client?.routines?.list?.().catch(() => ({ routines: [] })) ??
      Promise.resolve({ routines: [] }),
    refetchInterval: 20000,
  });
  const runsQuery = useQuery({
    queryKey: ["automations", "runs"],
    queryFn: () =>
      client?.routines?.listRuns?.(undefined, 20).catch(() => ({ runs: [] })) ??
      Promise.resolve({ runs: [] }),
    refetchInterval: 9000,
  });
  const packsQuery = useQuery({
    queryKey: ["automations", "packs"],
    queryFn: () =>
      client?.packs?.list?.().catch(() => ({ packs: [] })) ?? Promise.resolve({ packs: [] }),
    refetchInterval: 30000,
  });

  const runNowMutation = useMutation({
    mutationFn: (id: string) => client?.routines?.runNow?.(id),
    onSuccess: async () => {
      toast("ok", "Routine triggered.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const routines = toArray(routinesQuery.data, "routines");
  const runs = toArray(runsQuery.data, "runs");
  const packs = toArray(packsQuery.data, "packs");

  const statusColor = (status: string) => {
    const s = String(status || "").toLowerCase();
    if (s === "active" || s === "completed" || s === "done") return "tcp-badge-ok";
    if (s === "running" || s === "in_progress") return "tcp-badge-warn";
    if (s === "failed" || s === "error") return "tcp-badge-err";
    return "tcp-badge-info";
  };

  return (
    <div className="grid gap-4">
      {/* Installed packs from pack_builder */}
      {packs.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Installed Packs
          </p>
          {packs.map((pack: any, i: number) => (
            <div key={String(pack?.id || pack?.name || i)} className="tcp-list-item">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <span>📦</span>
                  <strong>{String(pack?.name || pack?.id || "Pack")}</strong>
                </div>
                <span className="tcp-badge-info">{String(pack?.version || "1.0.0")}</span>
              </div>
              <div className="tcp-subtle text-xs mt-1">
                {String(pack?.description || pack?.path || "")}
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {/* Scheduled routines */}
      {routines.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Scheduled Routines
          </p>
          {routines.map((routine: any) => {
            const id = String(routine?.id || routine?.routine_id || "");
            return (
              <div key={id} className="tcp-list-item">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <span>⏰</span>
                    <strong>{String(routine?.name || id || "Routine")}</strong>
                  </div>
                  <span className={statusColor(routine?.status)}>
                    {String(routine?.status || "active")}
                  </span>
                </div>
                <div className="tcp-subtle text-xs">{String(routine?.schedule || "manual")}</div>
                <div className="mt-2">
                  <button
                    className="tcp-btn h-7 px-2 text-xs"
                    onClick={() => runNowMutation.mutate(id)}
                  >
                    ▶ Run now
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      ) : null}

      {/* Recent run history */}
      {runs.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">Recent Runs</p>
          {runs.slice(0, 12).map((run: any, index: number) => (
            <div key={String(run?.run_id || run?.id || index)} className="tcp-list-item">
              <div className="flex items-center justify-between gap-2">
                <span className="font-medium text-sm">
                  {String(run?.name || run?.automation_id || run?.routine_id || "Run")}
                </span>
                <span className={statusColor(run?.status)}>{String(run?.status || "unknown")}</span>
              </div>
              <div className="tcp-subtle text-xs mt-1">{String(run?.run_id || run?.id || "")}</div>
            </div>
          ))}
        </div>
      ) : null}

      {!routines.length && !packs.length ? (
        <EmptyState text="No automations yet. Create your first one with the wizard!" />
      ) : null}
    </div>
  );
}

// ─── Spawn Approvals ────────────────────────────────────────────────────────

function SpawnApprovals({ client, toast }: { client: any; toast: any }) {
  const queryClient = useQueryClient();

  const approvalsQuery = useQuery({
    queryKey: ["automations", "approvals"],
    queryFn: () =>
      client?.agentTeams?.listApprovals?.().catch(() => ({ spawnApprovals: [] })) ??
      Promise.resolve({ spawnApprovals: [] }),
    refetchInterval: 6000,
  });

  const instancesQuery = useQuery({
    queryKey: ["automations", "instances"],
    queryFn: () =>
      client?.agentTeams?.listInstances?.().catch(() => ({ instances: [] })) ??
      Promise.resolve({ instances: [] }),
    refetchInterval: 8000,
  });

  const replyMutation = useMutation({
    mutationFn: ({ requestId, decision }: { requestId: string; decision: "approve" | "deny" }) =>
      client?.agentTeams?.replyApproval?.(requestId, decision),
    onSuccess: async () => {
      toast("ok", "Approval updated.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const approvals = toArray(approvalsQuery.data, "spawnApprovals");
  const instances = toArray(instancesQuery.data, "instances");

  return (
    <div className="grid gap-4">
      {approvals.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Pending Approvals
          </p>
          {approvals.map((approval: any, index: number) => {
            const requestId = String(approval?.request_id || approval?.id || `request-${index}`);
            return (
              <div key={requestId} className="tcp-list-item border-amber-500/40">
                <div className="mb-1 font-medium text-amber-300">
                  ⚠️ {String(approval?.reason || approval?.title || "Spawn request")}
                </div>
                <div className="tcp-subtle text-xs">{requestId}</div>
                <div className="mt-2 flex gap-2">
                  <button
                    className="tcp-btn-primary h-7 px-2 text-xs"
                    onClick={() => replyMutation.mutate({ requestId, decision: "approve" })}
                  >
                    ✓ Approve
                  </button>
                  <button
                    className="tcp-btn-danger h-7 px-2 text-xs"
                    onClick={() => replyMutation.mutate({ requestId, decision: "deny" })}
                  >
                    ✗ Deny
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      ) : null}

      {instances.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Active Agent Teams
          </p>
          {instances.map((instance: any, index: number) => (
            <div
              key={String(instance?.instance_id || instance?.id || index)}
              className="tcp-list-item"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <span>👥</span>
                  <strong>
                    {String(
                      instance?.name || instance?.template_id || instance?.instance_id || "Instance"
                    )}
                  </strong>
                </div>
                <span className="tcp-badge-info">{String(instance?.status || "active")}</span>
              </div>
              <div className="tcp-subtle text-xs mt-1">
                Mission: {String(instance?.mission_id || "—")}
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {!approvals.length && !instances.length ? (
        <EmptyState text="No pending approvals or active team instances." />
      ) : null}
    </div>
  );
}

// ─── Main Page ──────────────────────────────────────────────────────────────

export function AutomationsPage({ client, toast }: AppPageProps) {
  const [tab, setTab] = useState<ActiveTab>("create");

  const tabs: { id: ActiveTab; label: string; icon: string }[] = [
    { id: "create", label: "Create New", icon: "✨" },
    { id: "list", label: "My Automations", icon: "📋" },
    { id: "approvals", label: "Teams & Approvals", icon: "👥" },
  ];

  return (
    <div className="grid gap-4">
      {/* Tab bar */}
      <div className="flex gap-1 rounded-xl border border-slate-700/50 bg-slate-900/40 p-1">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`flex flex-1 items-center justify-center gap-1.5 rounded-lg px-3 py-2 text-sm font-medium transition-all ${
              tab === t.id
                ? "bg-amber-500/20 text-amber-300 shadow-sm"
                : "text-slate-400 hover:text-slate-200"
            }`}
          >
            <span>{t.icon}</span>
            <span>{t.label}</span>
          </button>
        ))}
      </div>

      {/* Tab content */}
      <AnimatePresence mode="wait">
        <motion.div
          key={tab}
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -6 }}
          transition={{ duration: 0.15 }}
        >
          {tab === "create" ? (
            <PageCard
              title="Create an Automation"
              subtitle="Describe what you want, pick a schedule, and Tandem handles the rest"
            >
              <CreateWizard client={client} toast={toast} />
            </PageCard>
          ) : tab === "list" ? (
            <PageCard title="My Automations" subtitle="Installed packs, routines and run history">
              <MyAutomations client={client} toast={toast} />
            </PageCard>
          ) : (
            <PageCard
              title="Teams & Approvals"
              subtitle="Active agent teams and pending spawn approvals"
            >
              <SpawnApprovals client={client} toast={toast} />
            </PageCard>
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
