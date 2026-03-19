export function mapOrchestratorPath(pathname) {
  const path = String(pathname || "").trim();
  if (path.startsWith("/api/orchestrator")) {
    return `/api/swarm${path.slice("/api/orchestrator".length)}`;
  }
  return path;
}

function toNumber(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function pickNumeric(...values) {
  for (const value of values) {
    const parsed = Number(value);
    if (Number.isFinite(parsed) && parsed > 0) return parsed;
  }
  return 0;
}

const DERIVED_MAX_ITERATIONS = 500;
const DERIVED_MAX_TOKENS = 400000;
const DERIVED_MAX_WALL_TIME_SECS = 7 * 24 * 60 * 60;
const DERIVED_MAX_SUBAGENT_RUNS = 2000;

function hasExplicitBudgetLimits(run) {
  return (
    pickNumeric(
      run?.budget?.max_iterations,
      run?.config?.max_iterations,
      run?.budget?.max_tokens,
      run?.budget?.max_total_tokens,
      run?.config?.max_total_tokens,
      run?.budget?.max_wall_time_secs,
      run?.config?.max_wall_time_secs,
      run?.budget?.max_subagent_runs,
      run?.config?.max_subagent_runs
    ) > 0
  );
}

export function deriveRunBudget(run, events, tasks) {
  const startedAtMs = toNumber(run?.started_at_ms);
  const updatedAtMs = toNumber(run?.updated_at_ms || Date.now());
  const wallTimeSecs =
    startedAtMs > 0 && updatedAtMs >= startedAtMs
      ? Math.round((updatedAtMs - startedAtMs) / 1000)
      : 0;
  const iterationsUsed = Array.isArray(events)
    ? events.filter((row) =>
        String(row?.type || "")
          .toLowerCase()
          .includes("task_")
      ).length
    : 0;
  const tokenEvents = Array.isArray(events) ? events : [];
  let tokensUsed = 0;
  for (const event of tokenEvents) {
    const payload = event?.payload && typeof event.payload === "object" ? event.payload : {};
    const total = pickNumeric(
      payload?.total_tokens,
      payload?.tokens_total,
      payload?.token_count,
      payload?.usage_total_tokens
    );
    if (total > 0) tokensUsed = Math.max(tokensUsed, total);
    const prompt = toNumber(payload?.prompt_tokens || payload?.input_tokens);
    const completion = toNumber(payload?.completion_tokens || payload?.output_tokens);
    if (prompt + completion > tokensUsed) tokensUsed = prompt + completion;
  }
  const explicitBudget = hasExplicitBudgetLimits(run);
  const maxIterations = pickNumeric(
    run?.budget?.max_iterations,
    run?.config?.max_iterations,
    DERIVED_MAX_ITERATIONS
  );
  const maxTokens = pickNumeric(
    run?.budget?.max_tokens,
    run?.budget?.max_total_tokens,
    run?.config?.max_total_tokens,
    DERIVED_MAX_TOKENS
  );
  const maxWallTimeSecs = pickNumeric(
    run?.budget?.max_wall_time_secs,
    run?.config?.max_wall_time_secs,
    DERIVED_MAX_WALL_TIME_SECS
  );
  const maxSubagentRuns = pickNumeric(
    run?.budget?.max_subagent_runs,
    run?.config?.max_subagent_runs,
    Math.max(DERIVED_MAX_SUBAGENT_RUNS, tasks.length * 6)
  );
  const subagentRunsUsed = Array.isArray(events)
    ? events.filter((row) =>
        String(row?.type || "")
          .toLowerCase()
          .includes("task_completed")
      ).length
    : 0;
  const measuredExceeded =
    iterationsUsed >= maxIterations ||
    tokensUsed >= maxTokens ||
    wallTimeSecs >= maxWallTimeSecs ||
    subagentRunsUsed >= maxSubagentRuns;
  const exceeded = explicitBudget
    ? Boolean(run?.budget?.exceeded) || measuredExceeded
    : Boolean(run?.budget?.exceeded);
  const exceededReason = explicitBudget
    ? String(
        run?.budget?.exceeded_reason || (exceeded ? "One or more execution limits exceeded." : "")
      )
    : "";
  return {
    max_iterations: maxIterations,
    iterations_used: iterationsUsed,
    max_tokens: maxTokens,
    tokens_used: tokensUsed,
    max_wall_time_secs: maxWallTimeSecs,
    wall_time_secs: wallTimeSecs,
    max_subagent_runs: maxSubagentRuns,
    subagent_runs_used: subagentRunsUsed,
    exceeded,
    exceeded_reason: exceededReason,
    limits_enforced: explicitBudget,
    source: explicitBudget ? "run" : "derived",
  };
}

export function inferStatusFromEvents(status, events) {
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
  if (normalized && normalized !== "planning") return normalized;
  const rows = Array.isArray(events) ? events : [];
  let sawPlanReady = false;
  let sawPlanApproved = false;
  for (const row of rows) {
    const type = String(row?.type || "")
      .trim()
      .toLowerCase();
    if (type === "plan_ready_for_approval") sawPlanReady = true;
    if (type === "plan_approved") sawPlanApproved = true;
  }
  if (sawPlanReady && !sawPlanApproved) return "awaiting_approval";
  return normalized || "idle";
}
