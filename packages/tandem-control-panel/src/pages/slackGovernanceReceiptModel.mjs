// TAN-686: pure data-shaping for the Slack Governance Receipts page.
//
// Kept as a plain module (no React, no TS-only syntax) so `node --test`
// exercises exactly the code the page ships, against captured payloads from
// the production `/context/runs` + governance-evidence APIs that the
// tandem-server ACME five-profile E2E persists.

function safeString(value, fallback = "") {
  const text = String(value ?? "").trim();
  return text || fallback;
}

export function runIdOf(run) {
  return safeString(run?.run_id || run?.runID || run?.id);
}

/** Slack identity persisted on a channel-originated context run. */
export function slackIdentityOf(run) {
  const metadata = run?.source_metadata;
  if (!metadata || typeof metadata !== "object") return null;
  const identity = {
    teamId: safeString(metadata.slack_team_id),
    appId: safeString(metadata.slack_app_id),
    channelId: safeString(metadata.slack_channel_id),
    userId: safeString(metadata.user_id),
    threadTs: safeString(metadata.slack_thread_ts),
    scopeId: safeString(metadata.scope_id),
  };
  if (!identity.userId && !identity.channelId) return null;
  return identity;
}

/** Only Slack-originated session runs qualify as receipts on this page. */
export function slackReceiptRuns(runs) {
  return (Array.isArray(runs) ? runs : []).filter(
    (run) =>
      safeString(run?.source_client).toLowerCase() === "channel:slack" &&
      slackIdentityOf(run) !== null
  );
}

/** Human-scannable selector label: requester and channel first, id last. */
export function receiptOptionLabel(run) {
  const identity = slackIdentityOf(run);
  const id = runIdOf(run);
  if (!identity) return id;
  const status = safeString(run?.status);
  const parts = [identity.userId || "unknown user", identity.channelId || "unknown channel"];
  if (status) parts.push(status);
  return `${parts.join(" · ")} · ${id}`;
}

const EVIDENCE_SECTIONS = [
  ["actors", (pkg) => pkg?.actors && typeof pkg.actors === "object"],
  ["tool_manifest", (pkg) => pkg?.tool_manifest && typeof pkg.tool_manifest === "object"],
  ["policy_decisions", (pkg) => Array.isArray(pkg?.policy_decisions)],
  ["approvals", (pkg) => pkg?.approvals && typeof pkg.approvals === "object"],
  ["memory_audit", (pkg) => Array.isArray(pkg?.memory_audit)],
  ["protected_audit", (pkg) => Array.isArray(pkg?.audit?.protected_events)],
  ["final_outcome", (pkg) => pkg?.final_outcome && typeof pkg.final_outcome === "object"],
];

/**
 * Honest evidence accounting: which receipt sections are absent from the
 * package (as opposed to present-but-empty, which is a legitimate result).
 * `null`/undefined package means the export itself was unavailable.
 */
export function evidenceCompleteness(pkg) {
  if (!pkg || typeof pkg !== "object" || Array.isArray(pkg)) {
    return { available: false, missing: EVIDENCE_SECTIONS.map(([name]) => String(name)) };
  }
  const missing = EVIDENCE_SECTIONS.filter(([, present]) => !present(pkg)).map(([name]) =>
    String(name)
  );
  return { available: true, missing };
}

/**
 * End-to-end identity correlation for one receipt: Slack identities from the
 * persisted run, run/context-run ids from the run and evidence package. The
 * page renders these side by side so a mismatch is visible, not papered over.
 */
export function receiptCorrelation(run, pkg) {
  const identity = slackIdentityOf(run) || {};
  const packageIdentity =
    (pkg && slackIdentityOf({ source_metadata: pkg?.run?.source_metadata })) || null;
  return {
    contextRunId: runIdOf(run),
    packageContextRunId: safeString(pkg?.run?.context_run_id),
    slack: identity,
    packageSlack: packageIdentity,
    identityConsistent:
      !packageIdentity ||
      (packageIdentity.userId === identity.userId &&
        packageIdentity.channelId === identity.channelId),
  };
}
