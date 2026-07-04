type ConnectorSuggestionPanelProps = {
  planPackage: any | null;
};

type ConnectorResolutionEntry = {
  capability?: string;
  why?: string | null;
  required?: boolean;
  degraded_mode_allowed?: boolean;
  resolved?: boolean;
  status?: string;
  binding_type?: string | null;
  binding_id?: string | null;
  allowlist_pattern?: string | null;
};

type ConnectorIntent = {
  capability?: string;
  why?: string;
  required?: boolean;
  degraded_mode_allowed?: boolean;
};

function safeString(value: unknown) {
  return String(value || "").trim();
}

function toArray(value: unknown) {
  return Array.isArray(value) ? value : [];
}

function statusBadge(entry: ConnectorResolutionEntry) {
  if (entry.resolved) return <span className="tcp-badge-success">mapped</span>;
  if (entry.required) return <span className="tcp-badge-warning">required</span>;
  return <span className="tcp-badge-info">optional</span>;
}

function actionLabel(entry: ConnectorResolutionEntry) {
  if (entry.resolved) return "mapped";
  if (entry.required) return "bind now";
  if (entry.degraded_mode_allowed) return "safe to defer";
  return "optional";
}

function actionTone(entry: ConnectorResolutionEntry) {
  if (entry.resolved) return "success";
  if (entry.required) return "warning";
  return "info";
}

function actionBadge(entry: ConnectorResolutionEntry) {
  const tone = actionTone(entry);
  const className =
    tone === "success"
      ? "tcp-badge-success"
      : tone === "warning"
        ? "tcp-badge-warning"
        : "tcp-badge-info";
  return <span className={className}>{actionLabel(entry)}</span>;
}

function intentKey(intent: ConnectorIntent, index: number) {
  return safeString(intent?.capability || `intent-${index + 1}`);
}

function summaryValue(value: unknown) {
  if (value === null || value === undefined) return "n/a";
  if (typeof value === "boolean") return value ? "yes" : "no";
  return String(value);
}

function buildResolutionEntries(planPackage: any | null) {
  const intents = toArray(planPackage?.connector_intents) as ConnectorIntent[];
  const resolutionEntries = toArray(
    planPackage?.connector_binding_resolution?.entries
  ) as ConnectorResolutionEntry[];
  const intentMap = new Map(intents.map((intent) => [safeString(intent?.capability), intent]));
  const seen = new Set<string>();

  const rows = resolutionEntries.length
    ? resolutionEntries.map((entry: ConnectorResolutionEntry) => {
        const capability = safeString(entry?.capability);
        const intent = intentMap.get(capability);
        seen.add(capability);
        return {
          capability,
          intent,
          entry,
        };
      })
    : intents.map((intent, index) => {
        const capability = intentKey(intent, index);
        return {
          capability,
          intent,
          entry: {
            capability,
            why: intent?.why,
            required: intent?.required,
            degraded_mode_allowed: intent?.degraded_mode_allowed,
            resolved: false,
            status: intent?.required ? "unresolved_required" : "unresolved_optional",
            binding_type: null,
            binding_id: null,
            allowlist_pattern: null,
          } as ConnectorResolutionEntry,
        };
      });

  intents.forEach((intent) => {
    const capability = safeString(intent?.capability);
    if (!capability || seen.has(capability)) return;
    rows.push({
      capability,
      intent,
      entry: {
        capability,
        why: intent?.why,
        required: intent?.required,
        degraded_mode_allowed: intent?.degraded_mode_allowed,
        resolved: false,
        status: intent?.required ? "unresolved_required" : "unresolved_optional",
        binding_type: null,
        binding_id: null,
        allowlist_pattern: null,
      },
    });
  });

  return rows.sort((left, right) => {
    const requiredScore = Number(right.entry.required) - Number(left.entry.required);
    if (requiredScore) return requiredScore;
    const resolvedScore = Number(left.entry.resolved) - Number(right.entry.resolved);
    if (resolvedScore) return resolvedScore;
    return left.capability.localeCompare(right.capability);
  });
}

function groupRows(
  rows: Array<{ capability: string; intent?: ConnectorIntent; entry: ConnectorResolutionEntry }>
) {
  const required = rows.filter((row) => row.entry.required);
  const optional = rows.filter((row) => !row.entry.required);
  return { required, optional };
}

export function ConnectorSuggestionPanel({ planPackage }: ConnectorSuggestionPanelProps) {
  if (!planPackage) return null;

  const rows = buildResolutionEntries(planPackage);
  const { required, optional } = groupRows(rows);
  const mappedCount = rows.filter((row) => row.entry.resolved).length;
  const unresolvedRequiredCount = rows.filter(
    (row) => row.entry.required && !row.entry.resolved
  ).length;
  const unresolvedOptionalCount = rows.filter(
    (row) => !row.entry.required && !row.entry.resolved
  ).length;

  return (
    <div className="grid gap-2">
      <div className="font-medium text-slate-200">Connector suggestions</div>
      <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
        <div className="grid gap-2 sm:grid-cols-4">
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">required intents</div>
            <div className="mt-1 text-slate-100">{required.length}</div>
          </div>
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">mapped</div>
            <div className="mt-1 text-slate-100">{mappedCount}</div>
          </div>
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
              unresolved required
            </div>
            <div className="mt-1 text-slate-100">{unresolvedRequiredCount}</div>
          </div>
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
              unresolved optional
            </div>
            <div className="mt-1 text-slate-100">{unresolvedOptionalCount}</div>
          </div>
        </div>
      </div>

      <div className="grid gap-3">
        <div className="grid gap-2">
          <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">Required</div>
          {required.length ? (
            required.map(({ capability, intent, entry }) => (
              <div
                key={`required-${capability}`}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">{capability}</div>
                  <div className="flex flex-wrap items-center gap-2">
                    {statusBadge(entry)}
                    {actionBadge(entry)}
                  </div>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">why</div>
                    <div className="mt-1 break-words text-slate-100">
                      {safeString(intent?.why || entry?.why || "n/a")}
                    </div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      operator action
                    </div>
                    <div className="mt-1 text-slate-100">
                      {entry.resolved
                        ? "Connector is already mapped."
                        : "Add a binding before activation."}
                    </div>
                  </div>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-3">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      binding type
                    </div>
                    <div className="mt-1 text-slate-100">{summaryValue(entry?.binding_type)}</div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">binding id</div>
                    <div className="mt-1 text-slate-100">{summaryValue(entry?.binding_id)}</div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      allowlist pattern
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {summaryValue(entry?.allowlist_pattern)}
                    </div>
                  </div>
                </div>
              </div>
            ))
          ) : (
            <div className="tcp-subtle text-xs">No required connector intents are declared.</div>
          )}
        </div>

        <div className="grid gap-2">
          <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">Optional</div>
          {optional.length ? (
            optional.map(({ capability, intent, entry }) => (
              <div
                key={`optional-${capability}`}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">{capability}</div>
                  <div className="flex flex-wrap items-center gap-2">
                    {statusBadge(entry)}
                    {actionBadge(entry)}
                  </div>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">why</div>
                    <div className="mt-1 break-words text-slate-100">
                      {safeString(intent?.why || entry?.why || "n/a")}
                    </div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      operator action
                    </div>
                    <div className="mt-1 text-slate-100">
                      {entry.resolved
                        ? "Connector is already mapped."
                        : entry.degraded_mode_allowed
                          ? "Optional now; can stay unbound in degraded mode."
                          : "Optional now; bind only if the workflow needs it."}
                    </div>
                  </div>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-3">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      degraded mode
                    </div>
                    <div className="mt-1 text-slate-100">
                      {summaryValue(entry?.degraded_mode_allowed)}
                    </div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      binding type
                    </div>
                    <div className="mt-1 text-slate-100">{summaryValue(entry?.binding_type)}</div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">binding id</div>
                    <div className="mt-1 text-slate-100">{summaryValue(entry?.binding_id)}</div>
                  </div>
                </div>
              </div>
            ))
          ) : (
            <div className="tcp-subtle text-xs">No optional connector intents are declared.</div>
          )}
        </div>
      </div>

      <div className="tcp-subtle tcp-text-caption">
        Connector suggestions are read-only and derived from the current plan package.
      </div>
    </div>
  );
}
