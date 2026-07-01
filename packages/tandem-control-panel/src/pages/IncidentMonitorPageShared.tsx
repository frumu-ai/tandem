import { Badge } from "../ui/index.tsx";

type AnyRecord = Record<string, any>;

function asRecord(value: unknown): AnyRecord {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as AnyRecord) : {};
}

function nestedRecord(record: AnyRecord, key: string): AnyRecord {
  return asRecord(record[key]);
}

function firstString(record: AnyRecord, keys: string[], fallback = ""): string {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value.trim();
    if (typeof value === "number" && Number.isFinite(value)) return String(value);
  }
  return fallback;
}

function detailContains(record: AnyRecord, pattern: RegExp): boolean {
  return pattern.test(String(record.detail || ""));
}

function valuesFromUnknown(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value
      .map((item) =>
        typeof item === "string" || typeof item === "number" ? String(item).trim() : ""
      )
      .filter(Boolean);
  }
  if (typeof value === "string" && value.trim()) return [value.trim()];
  if (typeof value === "number" && Number.isFinite(value)) return [String(value)];
  return [];
}

function collectValues(record: AnyRecord, keys: string[]): string[] {
  const payload = nestedRecord(record, "event_payload");
  const values: string[] = [];
  for (const key of keys) {
    values.push(...valuesFromUnknown(record[key]));
    values.push(...valuesFromUnknown(payload[key]));
  }
  return Array.from(new Set(values));
}

function shortValue(value: unknown, maxLength = 120): string {
  const text = String(value || "").trim();
  if (!text) return "";
  return text.length > maxLength ? `${text.slice(0, maxLength - 3)}...` : text;
}

function metadataValue(record: AnyRecord, keys: string[], detailPattern?: RegExp): string {
  const payload = nestedRecord(record, "event_payload");
  return (
    firstString(record, keys) ||
    firstString(payload, keys) ||
    (detailPattern && detailContains(record, detailPattern) ? "recorded in detail" : "")
  );
}

function linkHref(value: string): string | undefined {
  return /^https?:\/\//i.test(value) ? value : undefined;
}

function githubRepoIssueUrl(record: AnyRecord): string {
  const explicit = firstString(record, ["github_issue_url", "issue_url"]);
  if (explicit) return explicit;
  const repo = firstString(record, ["repo"]);
  const issueNumber = firstString(record, ["issue_number", "matched_issue_number"]);
  if (/^[^/\s]+\/[^/\s]+$/.test(repo) && /^\d+$/.test(issueNumber)) {
    return `https://github.com/${repo}/issues/${issueNumber}`;
  }
  return "";
}

function githubCommentUrl(record: AnyRecord): string {
  return firstString(record, ["github_comment_url", "comment_url"]);
}

function githubIssueLabel(record: AnyRecord): string {
  const issueNumber = firstString(record, ["issue_number", "matched_issue_number"]);
  return issueNumber ? `Issue #${issueNumber}` : "GitHub issue";
}

function statusTone(status: unknown): "ok" | "warn" | "err" | "info" | "ghost" {
  const normalized = String(status || "").toLowerCase();
  if (["posted", "published", "ok", "ready", "draft_ready", "open"].includes(normalized)) {
    return "ok";
  }
  if (["denied", "failed", "error", "blocked"].some((term) => normalized.includes(term))) {
    return "err";
  }
  if (["approval", "pending", "queued", "paused"].some((term) => normalized.includes(term))) {
    return "warn";
  }
  return normalized ? "info" : "ghost";
}

function GitHubLinks({ record }: { record: AnyRecord }) {
  const issueUrl = githubRepoIssueUrl(record);
  const commentUrl = githubCommentUrl(record);
  if (!issueUrl && !commentUrl) return null;
  return (
    <div className="mt-3 flex flex-wrap gap-2">
      {issueUrl ? (
        <a className="tcp-btn h-8 px-3 text-xs" href={issueUrl} target="_blank" rel="noreferrer">
          <i data-lucide="external-link"></i>
          {githubIssueLabel(record)}
        </a>
      ) : null}
      {commentUrl ? (
        <a className="tcp-btn h-8 px-3 text-xs" href={commentUrl} target="_blank" rel="noreferrer">
          <i data-lucide="message-square"></i>
          GitHub comment
        </a>
      ) : null}
    </div>
  );
}

function evidenceValues(record: AnyRecord): string[] {
  const evidence = [
    ...collectValues(record, [
      "evidence_refs",
      "artifact_refs",
      "artifactRefs",
      "files_touched",
      "filesTouched",
    ]),
    ...valuesFromUnknown(record.evidence_digest),
    ...valuesFromUnknown(record.response_excerpt),
    ...valuesFromUnknown(record.excerpt),
  ];
  if (
    detailContains(
      record,
      /(^|\n)(artifact_refs|evidence_refs|files_touched|tool_result_excerpt|excerpt):/i
    )
  ) {
    evidence.push("structured evidence in detail");
  }
  return Array.from(new Set(evidence)).filter(Boolean);
}

function lifecycleArtifactValues(record: AnyRecord): string[] {
  const artifacts = [
    ...collectValues(record, [
      "artifact_refs",
      "artifactRefs",
      "evidence_refs",
      "triage_artifacts",
      "research_sources",
      "related_failure_patterns",
    ]),
    ...valuesFromUnknown(record.triage_summary_artifact),
    ...valuesFromUnknown(record.issue_draft_artifact),
    ...valuesFromUnknown(record.duplicate_matches_artifact),
  ];
  for (const key of [
    "triage_summary_artifact",
    "issue_draft_artifact",
    "duplicate_matches_artifact",
  ]) {
    const artifact = asRecord(record[key]);
    artifacts.push(...valuesFromUnknown(artifact.path));
    artifacts.push(...valuesFromUnknown(artifact.artifact_ref));
    artifacts.push(...valuesFromUnknown(artifact.id));
  }
  if (
    detailContains(
      record,
      /(^|\n)(artifact_refs|evidence_refs|research_sources|related_failure_patterns):/i
    )
  ) {
    artifacts.push("structured refs in detail");
  }
  return Array.from(new Set(artifacts)).filter(Boolean);
}

function memoryValues(record: AnyRecord): string[] {
  const values = [
    ...collectValues(record, [
      "memory_id",
      "memory_ids",
      "memory_ref",
      "memory_refs",
      "memory_pattern",
      "memory_patterns",
      "failure_pattern_memory",
      "regression_signal_memory",
      "related_failure_patterns",
    ]),
  ];
  if (
    detailContains(
      record,
      /(^|\n)(memory|failure_pattern|regression_signal|related_failure_patterns):/i
    )
  ) {
    values.push("memory references in detail");
  }
  return Array.from(new Set(values)).filter(Boolean);
}

type LifecycleItem = {
  label: string;
  value: string;
  tone?: "ok" | "warn" | "err" | "info" | "ghost";
  href?: string;
};

export function SignalLifecyclePanel({
  record,
  kind,
}: {
  record: AnyRecord;
  kind: "incident" | "draft" | "post";
}) {
  const artifacts = lifecycleArtifactValues(record);
  const memory = memoryValues(record);
  const issueUrl = githubRepoIssueUrl(record);
  const commentUrl = githubCommentUrl(record);
  const publishedUrl = issueUrl || commentUrl;
  const coderReadyGate = asRecord(record.coder_ready_gate);
  const qualityGate = asRecord(record.quality_gate);
  const proposalStatus =
    firstString(record, ["github_status", "proposal_status", "issue_draft_status"]) ||
    (record.issue_body || record.issue_draft_artifact ? "issue draft available" : "");
  const approvalState =
    firstString(record, ["approval_status", "approval_decision", "status"]) ||
    (kind === "draft" ? "awaiting review" : "");
  const intakeId = firstString(record, ["incident_id", "post_id", "id", "fingerprint"]);
  const items: LifecycleItem[] = [
    {
      label: "Signal",
      value: shortValue(
        intakeId || metadataValue(record, ["event_type", "source"], /(^|\n)(event|source):/i)
      ),
      tone: qualityGate.status ? statusTone(qualityGate.status) : "info",
    },
    {
      label: "Draft",
      value: shortValue(firstString(record, ["draft_id"]) || (kind === "draft" ? intakeId : "")),
      tone: firstString(record, ["draft_id"]) || kind === "draft" ? "info" : "ghost",
    },
    {
      label: "Triage",
      value: shortValue(
        firstString(record, ["triage_run_id", "triageRunId", "triage_context_run_id"])
      ),
      tone: firstString(record, ["triage_run_id", "triageRunId", "triage_context_run_id"])
        ? "ok"
        : "ghost",
    },
    {
      label: "Proposal",
      value: shortValue(proposalStatus),
      tone: proposalStatus ? statusTone(proposalStatus) : "ghost",
    },
    {
      label: "Coder-ready",
      value: shortValue(
        firstString(coderReadyGate, ["status"]) || (record.coder_ready === true ? "passed" : "")
      ),
      tone:
        record.coder_ready === true || coderReadyGate.status === "passed"
          ? "ok"
          : coderReadyGate.status
            ? "warn"
            : "ghost",
    },
    {
      label: "Approval",
      value: shortValue(approvalState),
      tone: statusTone(approvalState),
    },
    {
      label: "Published",
      value: shortValue(
        firstString(record, ["issue_number"])
          ? githubIssueLabel(record)
          : publishedUrl || firstString(record, ["comment_id", "github_post_id"])
      ),
      tone: publishedUrl || record.issue_number ? "ok" : "ghost",
      href: publishedUrl ? linkHref(publishedUrl) : undefined,
    },
    {
      label: "Artifacts",
      value: shortValue(artifacts.slice(0, 3).join(", ")),
      tone: artifacts.length ? "info" : "ghost",
    },
    {
      label: "Memory",
      value: shortValue(memory.slice(0, 3).join(", ")),
      tone: memory.length ? "info" : "ghost",
    },
  ].filter((item) => item.value);

  if (!items.length) return null;

  return (
    <div className="mt-3 rounded-md border border-sky-400/20 bg-sky-400/[0.04] p-3">
      <div className="tcp-subtle mb-2 text-xs uppercase tracking-[0.18em]">Signal lifecycle</div>
      <div className="grid gap-2 md:grid-cols-3">
        {items.map((item) => (
          <div key={item.label} className="min-w-0 rounded border border-white/10 bg-black/10 p-2">
            <div className="mb-1 flex items-center justify-between gap-2">
              <span className="tcp-subtle text-[0.68rem] uppercase tracking-[0.14em]">
                {item.label}
              </span>
              <Badge tone={item.tone || "info"}>
                {item.tone === "ghost" ? "none" : item.tone || "info"}
              </Badge>
            </div>
            {item.href ? (
              <a
                className="break-words text-xs text-sky-200 hover:text-sky-100"
                href={item.href}
                target="_blank"
                rel="noreferrer"
              >
                {item.value}
              </a>
            ) : (
              <div className="break-words text-xs text-white/80">{item.value}</div>
            )}
          </div>
        ))}
      </div>
      <GitHubLinks record={record} />
    </div>
  );
}

function signalQualityGates(record: AnyRecord): { label: string; ok: boolean }[] {
  const backendGate = asRecord(record.quality_gate);
  const backendGates = Array.isArray(backendGate.gates) ? backendGate.gates : [];
  if (backendGates.length) {
    return backendGates.map((gate, index) => {
      const gateRecord = asRecord(gate);
      return {
        label: firstString(gateRecord, ["label", "key"], `Gate ${index + 1}`),
        ok: Boolean(gateRecord.passed ?? gateRecord.ok),
      };
    });
  }
  const status = String(record.status || "").toLowerCase();
  const eventType = metadataValue(
    record,
    ["event_type", "event", "signal_type", "failure_type", "error_kind"],
    /(^|\n)(event|event_type|failure_type|error_kind):/i
  );
  const confidence = metadataValue(
    record,
    ["confidence", "signal_confidence", "root_cause_confidence"],
    /(^|\n)confidence:/i
  );
  const risk = metadataValue(record, ["risk_level", "risk"], /(^|\n)risk_level:/i);
  const destination = metadataValue(
    record,
    [
      "expected_destination",
      "github_status",
      "github_issue_url",
      "github_comment_url",
      "issue_url",
      "comment_url",
    ],
    /(^|\n)expected_destination:/i
  );
  const hasEvidence = evidenceValues(record).length > 0;
  const source = metadataValue(
    record,
    ["source", "component", "event_type"],
    /(^|\n)(source|component|event_type):/i
  );
  const hasDedupe =
    !!record.fingerprint || !!record.duplicate_summary || Array.isArray(record.duplicate_matches);
  const routineNoise =
    /progress|heartbeat|started|retrying|attempt\.started|minor_retry/.test(eventType) ||
    /progress|retrying/.test(status);
  const terminalOrActionable = eventType
    ? !routineNoise
    : /(failed|error|blocked|queued|draft|approval|posted|created|duplicate_suppressed)/.test(
        status
      );
  return [
    { label: "Source known", ok: !!source },
    { label: "Type classified", ok: !!eventType },
    { label: "Confidence recorded", ok: !!confidence },
    { label: "Dedupe/fingerprint checked", ok: hasDedupe },
    { label: "Evidence or artifact refs", ok: hasEvidence },
    { label: "Destination clear", ok: !!destination },
    { label: "Risk known", ok: !!risk },
    { label: "Not routine noise", ok: terminalOrActionable && !routineNoise },
  ];
}

export function SignalMetadataGrid({ record }: { record: AnyRecord }) {
  const evidence = evidenceValues(record);
  const qualityGate = asRecord(record.quality_gate);
  const duplicateCount = Array.isArray(record.duplicate_matches)
    ? record.duplicate_matches.length
    : 0;
  const rows = [
    [
      "Gate",
      qualityGate.status
        ? `${String(qualityGate.status)} (${Number(qualityGate.passed_count || 0)}/${Number(qualityGate.total_count || 0)})`
        : "",
    ],
    [
      "Source",
      metadataValue(
        record,
        ["source", "component", "event_type"],
        /(^|\n)(source|component|event_type):/i
      ),
    ],
    [
      "Type",
      metadataValue(
        record,
        ["event_type", "event", "signal_type", "failure_type", "error_kind"],
        /(^|\n)(event|event_type|failure_type|error_kind):/i
      ),
    ],
    [
      "Confidence",
      metadataValue(
        record,
        ["confidence", "signal_confidence", "root_cause_confidence"],
        /(^|\n)confidence:/i
      ),
    ],
    ["Risk", metadataValue(record, ["risk_level", "risk"], /(^|\n)risk_level:/i)],
    [
      "Destination",
      metadataValue(
        record,
        [
          "expected_destination",
          "github_status",
          "github_issue_url",
          "github_comment_url",
          "issue_url",
          "comment_url",
        ],
        /(^|\n)expected_destination:/i
      ),
    ],
    ["Fingerprint", firstString(record, ["fingerprint"])],
    ["Run", firstString(record, ["run_id", "runID"])],
    ["Session", firstString(record, ["session_id", "sessionID"])],
    ["Correlation", firstString(record, ["correlation_id", "correlationID"])],
    ["Evidence", evidence.slice(0, 3).join(", ")],
    [
      "Duplicates",
      duplicateCount
        ? `${duplicateCount} match${duplicateCount === 1 ? "" : "es"}`
        : record.duplicate_summary
          ? "summary available"
          : "",
    ],
  ].filter(([, value]) => value);

  if (!rows.length) return null;

  return (
    <div className="tcp-subtle mt-3 grid gap-1 rounded-md border border-white/10 bg-white/[0.03] p-3 text-xs md:grid-cols-2">
      {rows.map(([label, value]) => (
        <div key={label} className="min-w-0">
          <span className="text-white/60">{label}: </span>
          <span className="break-words text-white/80">{value}</span>
        </div>
      ))}
    </div>
  );
}

export function QualityGateStrip({ record }: { record: AnyRecord }) {
  const gates = signalQualityGates(record);
  const backendGate = asRecord(record.quality_gate);
  const isBackendGate = Array.isArray(backendGate.gates) && backendGate.gates.length > 0;
  const passed = gates.filter((gate) => gate.ok).length;
  return (
    <div className="mt-3 border-t border-white/10 pt-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div className="tcp-subtle text-xs uppercase tracking-[0.18em]">
          Signal quality gates{isBackendGate ? "" : " (heuristic)"}
        </div>
        <Badge tone={passed === gates.length ? "ok" : passed >= 5 ? "warn" : "err"}>
          {passed}/{gates.length}
        </Badge>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {gates.map((gate) => (
          <Badge key={gate.label} tone={gate.ok ? "ok" : "ghost"}>
            {gate.ok ? "ok" : "missing"} {gate.label}
          </Badge>
        ))}
      </div>
    </div>
  );
}

export function StatTile({
  label,
  value,
  tone = "info",
}: {
  label: string;
  value: any;
  tone?: any;
}) {
  return (
    <div className="tcp-list-item min-h-[5rem]">
      <div className="tcp-subtle text-xs uppercase tracking-[0.18em]">{label}</div>
      <div className="mt-2 flex items-center justify-between gap-2">
        <div className="min-w-0 truncate text-lg font-semibold">{value}</div>
        <Badge tone={tone}>{String(value || "n/a")}</Badge>
      </div>
    </div>
  );
}

export function QueryError({ error }: { error: unknown }) {
  const message = error instanceof Error ? error.message : String(error || "Unknown error");
  return (
    <div className="tcp-list-item border-rose-500/40">
      <div className="font-medium text-rose-200">Request failed</div>
      <div className="tcp-subtle mt-1 break-words text-xs">{message}</div>
    </div>
  );
}
