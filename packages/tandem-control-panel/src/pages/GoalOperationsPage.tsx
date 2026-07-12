import type { TandemClient } from "@frumu/tandem-client";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { GoalActionControls, type GoalActionInput } from "../features/orchestration-operations/GoalActionControls";
import { GoalOperationsCanvas } from "../features/orchestration-operations/GoalOperationsCanvas";
import { GoalOperationsInspector } from "../features/orchestration-operations/GoalOperationsInspector";
import { GoalReplayTimeline } from "../features/orchestration-operations/GoalReplayTimeline";
import type { GoalProjectionAction } from "../features/orchestration-operations/types";
import { useGoalProjection } from "../features/orchestration-operations/useGoalProjection";
import { Icon } from "../ui/Icon";
import type { AppPageProps } from "./pageTypes";
import "../features/orchestration-operations/goalOperations.css";

type ActionRuntime = TandemClient["statefulRuntime"] & {
  performGoalAction(
    goalId: string,
    actionId: string,
    input: {
      expectedUpdatedAtMs: number;
      idempotencyKey: string;
      reason?: string;
      decision?: string;
      payload?: Record<string, unknown>;
    }
  ): Promise<unknown>;
};

function goalIdFromHash() {
  const query = window.location.hash.split("?")[1] || "";
  return new URLSearchParams(query).get("goal_id") || new URLSearchParams(query).get("goalId") || "";
}

function formatNumber(value: unknown) {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(Number(value || 0));
}

function formatCost(value: unknown) {
  return new Intl.NumberFormat(undefined, { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(Number(value || 0));
}

function connectionLabel(connection: string) {
  if (connection === "live") return "Live connection";
  if (connection === "polling") return "Polling fallback";
  if (connection === "offline") return "Offline, reconnecting";
  return "Connecting";
}

export function GoalOperationsPage({ client, toast, navigate }: AppPageProps) {
  const [goalId, setGoalId] = useState(goalIdFromHash);
  useEffect(() => {
    const syncGoalId = () => setGoalId(goalIdFromHash());
    window.addEventListener("hashchange", syncGoalId);
    return () => window.removeEventListener("hashchange", syncGoalId);
  }, []);
  const goalsQuery = useQuery({
    queryKey: ["goal-operations", "goals"],
    queryFn: () => client.statefulRuntime.listGoals({ limit: 100 }),
    enabled: !goalId,
  });
  const { state, projection, connection, error, dispatch, setMode, scrub, refresh } = useGoalProjection(client, goalId);
  const [pendingActionId, setPendingActionId] = useState("");

  if (!goalId) {
    return (
      <main className="goal-ops-goal-picker">
        <header><div><span className="goal-ops-eyebrow">Runtime</span><h1>Goal Operations</h1></div>
          <button type="button" className="tcp-btn" onClick={() => navigate("orchestrations")}><Icon name="route" size={15} /> Orchestrations</button></header>
        {goalsQuery.isLoading ? <p className="goal-ops-muted">Loading goals...</p> : null}
        {goalsQuery.isError ? <div role="alert" className="goal-ops-reconnect">Goals could not be loaded. <button className="tcp-btn" onClick={() => void goalsQuery.refetch()}>Retry</button></div> : null}
        <div className="goal-ops-goal-list">
          {(goalsQuery.data?.goals || []).map((goal) => (
            <button key={goal.goal_id} onClick={() => {
              setGoalId(goal.goal_id);
              window.location.hash = `#/goal-operations?goal_id=${encodeURIComponent(goal.goal_id)}`;
            }}>
              <span><strong>{goal.objective}</strong><small>{goal.orchestration_id} · {goal.goal_id}</small></span>
              <span className={`goal-ops-goal-status status-${goal.status}`}>{goal.status.replaceAll("_", " ")}</span>
            </button>
          ))}
        </div>
        {!goalsQuery.isLoading && !goalsQuery.data?.goals.length ? <div className="goal-ops-empty-page"><Icon name="target" size={28} /><h2>No goals yet</h2><p>Start a published orchestration to monitor it here.</p></div> : null}
      </main>
    );
  }

  if (!projection) {
    return (
      <main className="goal-ops-empty-page" aria-live="polite">
        <Icon name={error ? "triangle-alert" : "loader-circle"} size={28} />
        <h1>{error ? "Goal projection unavailable" : "Loading goal operations"}</h1>
        <p>{error || "Connecting to the canonical goal projection."}</p>
        {error ? <button type="button" className="tcp-btn" onClick={() => void refresh()}>Retry connection</button> : null}
      </main>
    );
  }

  const goal = projection.goal;
  const budgets = projection.budgets;
  const performAction = async (action: GoalProjectionAction, input: GoalActionInput) => {
    setPendingActionId(action.id);
    try {
      const runtime = client.statefulRuntime as ActionRuntime;
      await runtime.performGoalAction(goalId, action.id, {
        expectedUpdatedAtMs: goal.updated_at_ms,
        idempotencyKey: crypto.randomUUID(),
        ...input,
      });
      await refresh();
      toast("ok", `${action.label} completed.`);
    } catch (reason) {
      toast("err", reason instanceof Error ? reason.message : `${action.label} failed.`);
    } finally {
      setPendingActionId("");
    }
  };

  return (
    <main className="goal-ops-page" data-mode={state.mode}>
      <header className="goal-ops-header">
        <div className="goal-ops-title-block">
          <button type="button" className="goal-ops-back" onClick={() => navigate("orchestrations")} aria-label="Back to orchestrations">
            <Icon name="chevron-left" size={17} />
          </button>
          <div>
            <div className="goal-ops-title-line">
              <h1>{goal.objective}</h1>
              <span className={`goal-ops-goal-status status-${goal.status}`}>{goal.status.replaceAll("_", " ")}</span>
            </div>
            <p>{projection.orchestration?.name || "Pinned orchestration unavailable"} · Goal {goal.goal_id}</p>
          </div>
        </div>
        <div className={`goal-ops-connection ${connection}`} role="status">
          <span aria-hidden="true" /> {connectionLabel(connection)}
        </div>
      </header>
      {error ? <div className="goal-ops-reconnect" role="alert"><Icon name="triangle-alert" size={15} /> {error} Last known state remains visible.</div> : null}
      {state.gapDetected ? <div className="goal-ops-repair" role="status">Timeline gap detected and repaired from the canonical projection.</div> : null}
      <section className="goal-ops-metrics" aria-label="Goal metrics">
        <div><span>Status</span><strong>{goal.status.replaceAll("_", " ")}</strong></div>
        <div><span>Hop progress</span><strong>{formatNumber(goal.hop_count)} / {formatNumber(goal.policy?.max_hops)}</strong></div>
        <div><span>Tokens</span><strong>{formatNumber(goal.total_tokens)}</strong><small>{budgets.remaining.tokens == null ? "No token limit" : `${formatNumber(budgets.remaining.tokens)} remaining`}</small></div>
        <div><span>Cost</span><strong>{formatCost(goal.total_cost_usd)}</strong><small>{budgets.remaining.cost_usd == null ? "No cost limit" : `${formatCost(budgets.remaining.cost_usd)} remaining`}</small></div>
        <div><span>Timeline</span><strong>{state.timeline.length}</strong><small>bounded events</small></div>
      </section>
      <div className="goal-ops-workspace">
        <section className="goal-ops-canvas-panel" aria-label="Orchestration progress">
          <GoalOperationsCanvas goalId={goalId} projection={projection} selection={state.selection} onSelect={(selection) => dispatch({ type: "select", selection })} />
        </section>
        <GoalOperationsInspector projection={projection} selection={state.selection} />
      </div>
      <div className="goal-ops-lower">
        <GoalReplayTimeline mode={state.mode} timeline={state.timeline} replayIndex={state.replayIndex} onModeChange={setMode} onScrub={(index) => void scrub(index)} />
        <GoalActionControls actions={projection.actions || []} mode={state.mode} pendingActionId={pendingActionId} onPerform={performAction} />
      </div>
    </main>
  );
}
