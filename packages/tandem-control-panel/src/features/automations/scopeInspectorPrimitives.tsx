import { formatJson } from "../../pages/ui";

export type ScopeView =
  | "all"
  | "scope"
  | "graph"
  | "compare"
  | "credentials"
  | "handoffs"
  | "runtime"
  | "audit";

export type HistoryVisibilityFilter = "all" | "routine_only" | "plan_owner" | "named_roles";

export type ArtifactVisibilityFilter =
  | "all"
  | "routine_only"
  | "declared_consumers"
  | "plan_owner"
  | "workspace";

export type RoutineGraphDependency = {
  routineId: string;
  dependencyType: string;
  mode: string;
  resolved: boolean;
};

export type RoutineGraphNode = {
  routineId: string;
  routine: any;
  index: number;
  layer: number;
  stepCount: number;
  downstreamCount: number;
  dependencies: RoutineGraphDependency[];
};

export type RoutineGraphLayer = {
  layer: number;
  items: RoutineGraphNode[];
};

export type ScopeInspectorProps = {
  planPackage: any | null;
  planPackageBundle?: any | null;
  planPackageReplay?: any | null;
  validationReport?: any | null;
  runtimeContext?: any | null;
  approvedPlanMaterialization?: any | null;
  overlapHistoryEntries?: any[] | null;
  title?: string;
  onOpenPromptEditor?: () => void;
  onOpenModelRoutingEditor?: () => void;
  onOpenConnectorBindingsEditor?: () => void;
  onReplaceSharedContextPack?: (fromPackId: string, toPackId: string) => void;
};

export function safeString(value: unknown) {
  return String(value || "").trim();
}

export function toArray(value: unknown) {
  return Array.isArray(value) ? value : [];
}

export function listPaths(values: unknown) {
  const rows = toArray(values)
    .map((entry) => safeString(entry))
    .filter(Boolean);
  if (!rows.length) return "none";
  return rows.join(", ");
}

export function kv(label: string, value: unknown) {
  return (
    <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
      <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">{label}</div>
      <div className="mt-1 break-words text-sm text-slate-100">{String(value || "n/a")}</div>
    </div>
  );
}

export function downloadJsonFile(filename: string, payload: unknown) {
  const blob = new Blob([JSON.stringify(payload, null, 2)], {
    type: "application/json;charset=utf-8",
  });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.rel = "noopener noreferrer";
  anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function historyVisibilityLabel(value: unknown) {
  const key = safeString(value).toLowerCase();
  if (key === "routine_only") return "Routine only";
  if (key === "plan_owner") return "Plan owner";
  if (key === "named_roles") return "Named roles";
  return "Unknown";
}

export function artifactVisibilityLabel(value: unknown) {
  const key = safeString(value).toLowerCase();
  if (key === "routine_only") return "Routine only";
  if (key === "declared_consumers") return "Declared consumers";
  if (key === "plan_owner") return "Plan owner";
  if (key === "workspace") return "Workspace";
  return "Unknown";
}

export function prettyEnumLabel(value: unknown) {
  const label = safeString(value);
  if (!label) return "n/a";
  return label
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

export function contextPackVisibilityLabel(value: unknown) {
  const key = safeString(value).toLowerCase();
  if (key === "project_allowlist") return "Project allowlist";
  if (key === "same_project") return "Same project";
  return prettyEnumLabel(value);
}

export function contextPackStateLabel(state: string, isStale: boolean) {
  const normalized = safeString(state).toLowerCase();
  if (normalized === "revoked") return "revoked";
  if (normalized === "superseded") return "superseded";
  if (normalized === "published" && isStale) return "stale";
  return normalized || "unknown";
}

export function contextPackStateTone(state: string, isStale: boolean) {
  const normalized = safeString(state).toLowerCase();
  if (normalized === "published" && !isStale) return "success";
  return "warning";
}

export function contextPackStateHint(entry: any) {
  if (!entry?.pack) return "";
  if (entry.pack.isStale) {
    return "Freshness window has elapsed; rebinding is recommended before reuse.";
  }
  if (entry.pack.state === "superseded") {
    const supersededBy = safeString(entry.pack.raw?.superseded_by_pack_id);
    return supersededBy
      ? `This shared workflow context has been superseded by ${supersededBy}. Rebind to the replacement before creating new runs.`
      : "This shared workflow context has been superseded. Rebind to the replacement before creating new runs.";
  }
  if (entry.pack.state !== "published") {
    return "This shared workflow context is not published. New runs will be blocked until a published context is selected.";
  }
  return "";
}

export function statusTone(value: unknown) {
  const status = safeString(value).toLowerCase();
  if (!status) return "info";
  if (
    [
      "ok",
      "ready",
      "complete",
      "completed",
      "defined",
      "satisfied",
      "met",
      "covered",
      "valid",
      "pass",
      "passing",
    ].includes(status)
  ) {
    return "success";
  }
  if (
    [
      "blocked",
      "missing",
      "incomplete",
      "unmet",
      "invalid",
      "unsatisfied",
      "failed",
      "error",
    ].includes(status)
  ) {
    return "warning";
  }
  return "info";
}

export function statusBadge(value: unknown) {
  const label = safeString(value) || "unknown";
  const tone = statusTone(value);
  const className =
    tone === "success"
      ? "tcp-badge-success"
      : tone === "warning"
        ? "tcp-badge-warning"
        : "tcp-badge-info";
  return <span className={className}>{label}</span>;
}

export function timestampLabel(value: unknown) {
  const timestamp = Number(value || 0);
  if (!Number.isFinite(timestamp) || timestamp <= 0) return "n/a";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(timestamp));
}

export function tokenizeSuggestionTerms(value: unknown) {
  return safeString(value)
    .toLowerCase()
    .split(/[^a-z0-9]+/g)
    .map((term) => term.trim())
    .filter((term) => term.length >= 4);
}

export function parseCommaSeparatedProjectKeys(value: string) {
  return Array.from(
    new Set(
      value
        .split(/[\n,]/g)
        .map((entry) => safeString(entry))
        .filter(Boolean)
    )
  );
}

export function packSuggestionReason(
  pack: any,
  currentSourcePlanId: string,
  currentTitleTerms: string[]
) {
  const reasons: string[] = [];
  if (currentSourcePlanId && pack.sourcePlanId === currentSourcePlanId) {
    reasons.push("same source plan");
  }
  const packTerms = new Set([
    ...tokenizeSuggestionTerms(pack.title),
    ...tokenizeSuggestionTerms(pack.raw?.summary),
  ]);
  const overlap = currentTitleTerms.filter((term) => packTerms.has(term));
  if (overlap.length) {
    reasons.push(`title overlap: ${overlap.slice(0, 2).join(", ")}`);
  }
  if (!pack.isStale && pack.updatedAtMs) {
    reasons.push("recent");
  }
  return reasons.length ? reasons.join(" · ") : "recent workspace context";
}

export function successCriteriaStatus(entry: any) {
  return (
    entry?.status ||
    entry?.coverage_status ||
    entry?.state ||
    entry?.readiness ||
    entry?.result ||
    entry?.evaluation ||
    null
  );
}

export function booleanBadge(value: unknown) {
  if (value === null || value === undefined) {
    return <span className="tcp-badge-info">n/a</span>;
  }
  const truthy = value === true || String(value).toLowerCase() === "true";
  return (
    <span className={truthy ? "tcp-badge-success" : "tcp-badge-warning"}>
      {truthy ? "true" : "false"}
    </span>
  );
}

export function approvalModeLabel(value: unknown) {
  const label = prettyEnumLabel(value);
  return label.toLowerCase() === "n/a" ? "n/a" : label;
}

export function formatPercentage(numerator: number, denominator: number) {
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || denominator <= 0) {
    return "n/a";
  }
  return `${Math.round((numerator / denominator) * 100)}%`;
}

export function planApprovalMatrixRows(matrix: any) {
  return [
    { key: "public_posts", label: "public posts", value: matrix?.public_posts },
    { key: "public_replies", label: "public replies", value: matrix?.public_replies },
    { key: "outbound_email", label: "outbound email", value: matrix?.outbound_email },
    { key: "internal_reports", label: "internal reports", value: matrix?.internal_reports },
    {
      key: "connector_mutations",
      label: "connector mutations",
      value: matrix?.connector_mutations,
    },
    {
      key: "destructive_actions",
      label: "destructive actions",
      value: matrix?.destructive_actions,
    },
  ];
}

export function diffValue(value: unknown) {
  if (value === null || value === undefined) return "n/a";
  if (typeof value === "string") return value;
  return formatJson(value);
}
