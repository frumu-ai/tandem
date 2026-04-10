import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { AnimatedPage, Badge, PageHeader, PanelCard, SplitView } from "../ui/index";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

const WORKFLOW_IMPORT_HANDOFF_KEY = "tandem.workflow.importHandoff.v1";

function safeString(value: unknown) {
  return String(value || "").trim();
}

function formatJson(value: unknown) {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return "{}";
  }
}

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function parseBundle(text: string) {
  const trimmed = safeString(text);
  if (!trimmed) {
    throw new Error("Paste a workflow bundle JSON object first.");
  }
  const value = JSON.parse(trimmed);
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Workflow bundle must be a JSON object.");
  }
  return value;
}

export function WorkflowsPage({ client, toast, navigate, identity }: AppPageProps) {
  const queryClient = useQueryClient();
  const [bundleText, setBundleText] = useState("");
  const [creatorId, setCreatorId] = useState(identity.botName || "workflow_planner");
  const [projectSlug, setProjectSlug] = useState("workflow-imports");
  const [title, setTitle] = useState("");
  const [selectedSessionId, setSelectedSessionId] = useState("");
  const [previewResult, setPreviewResult] = useState<any>(null);
  const [importError, setImportError] = useState("");

  const sessionsQuery = useQuery({
    queryKey: ["workflow-center", "sessions"],
    queryFn: () => client.workflowPlannerSessions.list().catch(() => ({ sessions: [] })),
    refetchInterval: 15000,
  });

  const sessions = useMemo(() => {
    return toArray(sessionsQuery.data, "sessions").sort((a: any, b: any) => {
      const right = Number(b?.updated_at_ms || 0);
      const left = Number(a?.updated_at_ms || 0);
      return right - left;
    });
  }, [sessionsQuery.data]);

  useEffect(() => {
    if (!selectedSessionId && sessions.length) {
      setSelectedSessionId(sessions[0].session_id);
    }
  }, [selectedSessionId, sessions]);

  const selectedSessionQuery = useQuery({
    queryKey: ["workflow-center", "session", selectedSessionId],
    enabled: !!selectedSessionId,
    queryFn: () =>
      client.workflowPlannerSessions.get(selectedSessionId).catch(() => ({ session: null })),
  });

  const selectedSession = selectedSessionQuery.data?.session || null;

  const previewMutation = useMutation({
    mutationFn: async () => {
      setImportError("");
      const bundle = parseBundle(bundleText);
      return client.workflowPlans.importPreview({
        bundle,
        creatorId,
        creator_id: creatorId,
        projectSlug,
        project_slug: projectSlug,
        title: safeString(title) || undefined,
      });
    },
    onSuccess: (payload) => {
      setPreviewResult(payload);
      toast("ok", "Import preview generated.");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setImportError(message);
      toast("err", message);
    },
  });

  const importMutation = useMutation({
    mutationFn: async () => {
      setImportError("");
      const bundle = parseBundle(bundleText);
      return client.workflowPlans.importPlan({
        bundle,
        creatorId,
        creator_id: creatorId,
        projectSlug,
        project_slug: projectSlug,
        title: safeString(title) || undefined,
      });
    },
    onSuccess: async (payload) => {
      const session = payload?.session;
      if (session?.session_id) {
        try {
          localStorage.setItem(
            WORKFLOW_IMPORT_HANDOFF_KEY,
            JSON.stringify({
              session_id: session.session_id,
              title: session.title,
              project_slug: session.project_slug,
              source_kind: session.source_kind || "imported_bundle",
              source_bundle_digest: session.source_bundle_digest || null,
              current_plan_id: session.current_plan_id || null,
            })
          );
        } catch {
          // Ignore storage failures.
        }
        setSelectedSessionId(session.session_id);
      }
      setPreviewResult(payload);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["workflow-center"] }),
        queryClient.invalidateQueries({ queryKey: ["intent-planner"] }),
      ]);
      toast("ok", "Workflow imported and saved.");
      navigate("planner");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setImportError(message);
      toast("err", message);
    },
  });

  const copySessionId = async (sessionId: string) => {
    try {
      await navigator.clipboard.writeText(sessionId);
      toast("ok", "Session id copied.");
    } catch {
      toast("warn", "Could not copy session id.");
    }
  };

  const openSelectedSession = () => {
    if (!selectedSessionId) return;
    try {
      const session = selectedSession;
      if (session) {
        localStorage.setItem(
          WORKFLOW_IMPORT_HANDOFF_KEY,
          JSON.stringify({
            session_id: session.session_id,
            title: session.title,
            project_slug: session.project_slug,
            source_kind: session.source_kind || "planner",
            source_bundle_digest: session.source_bundle_digest || null,
            current_plan_id: session.current_plan_id || null,
          })
        );
      }
    } catch {
      // Ignore storage failures.
    }
    navigate("planner");
  };

  const sessionSourceLabel = (session: any) => safeString(session?.source_kind || "planner");

  return (
    <AnimatedPage className="grid gap-4">
      <PageHeader
        eyebrow="Workflow center"
        title="Workflow sessions, imports, and provenance"
        subtitle="Import a bundle durably, inspect the stored session, and jump back to the planner when you need to revise or apply it."
        badges={
          <>
            <Badge tone="info">{sessions.length} sessions</Badge>
            <Badge tone="warn">
              {sessions.filter((session: any) => session?.source_kind === "imported_bundle").length}{" "}
              imports
            </Badge>
          </>
        }
      />

      <PanelCard
        title="Import workflow bundle"
        subtitle="Paste a workflow bundle JSON export, preview it, then save it as a durable planner session."
      >
        <div className="grid gap-3">
          <label className="grid gap-2 text-sm">
            <span className="tcp-subtle">Bundle JSON</span>
            <textarea
              className="min-h-56 w-full rounded-xl border border-white/10 bg-black/20 p-3 font-mono text-xs text-white outline-none"
              placeholder='{"plan_id":"...","mission":{...},"routine_graph":[...]}'
              value={bundleText}
              onChange={(event) => setBundleText(event.target.value)}
            />
          </label>
          <div className="grid gap-3 md:grid-cols-3">
            <label className="grid gap-2 text-sm">
              <span className="tcp-subtle">Creator id</span>
              <input
                className="tcp-input"
                value={creatorId}
                onChange={(event) => setCreatorId(event.target.value)}
              />
            </label>
            <label className="grid gap-2 text-sm">
              <span className="tcp-subtle">Project slug</span>
              <input
                className="tcp-input"
                value={projectSlug}
                onChange={(event) => setProjectSlug(event.target.value)}
              />
            </label>
            <label className="grid gap-2 text-sm">
              <span className="tcp-subtle">Session title</span>
              <input
                className="tcp-input"
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                placeholder="Optional"
              />
            </label>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className="tcp-btn-secondary"
              onClick={() => previewMutation.mutate()}
            >
              Preview import
            </button>
            <button
              type="button"
              className="tcp-btn-primary"
              onClick={() => importMutation.mutate()}
            >
              Import and open planner
            </button>
          </div>
          {importError ? <div className="text-sm text-red-300">{importError}</div> : null}
          {previewResult ? (
            <div className="grid gap-3 rounded-2xl border border-white/10 bg-black/20 p-4">
              <div className="flex flex-wrap items-center gap-2">
                <Badge tone={previewResult?.persisted ? "ok" : "info"}>
                  {previewResult?.persisted ? "persisted" : "preview only"}
                </Badge>
                <Badge tone="ghost">
                  {safeString(
                    previewResult?.summary?.plan_id ||
                      previewResult?.plan_package_preview?.plan_id ||
                      "unknown"
                  )}
                </Badge>
              </div>
              <pre className="max-h-64 overflow-auto rounded-xl bg-black/30 p-3 text-xs text-white/80">
                {formatJson(
                  previewResult?.plan_package_validation ||
                    previewResult?.import_validation ||
                    previewResult
                )}
              </pre>
            </div>
          ) : null}
        </div>
      </PanelCard>

      <SplitView
        main={
          <PanelCard
            title="Planner sessions"
            subtitle="Imported bundles, local drafts, and applied workflow sessions all show up here."
          >
            <div className="grid gap-2">
              {sessions.length ? (
                sessions.map((session: any) => {
                  const active = session.session_id === selectedSessionId;
                  const sourceKind = sessionSourceLabel(session);
                  return (
                    <button
                      key={session.session_id}
                      type="button"
                      className={`tcp-list-item text-left ${active ? "border-amber-400/70" : ""}`}
                      onClick={() => setSelectedSessionId(session.session_id)}
                    >
                      <div className="mb-1 flex items-center justify-between gap-2">
                        <strong>{safeString(session.title || session.session_id)}</strong>
                        <Badge tone={sourceKind === "imported_bundle" ? "warn" : "info"}>
                          {sourceKind}
                        </Badge>
                      </div>
                      <div className="flex flex-wrap gap-2 text-xs">
                        <span className="tcp-subtle">{safeString(session.project_slug)}</span>
                        <span className="tcp-subtle">
                          {safeString(session.source_bundle_digest || "")}
                        </span>
                        <span className="tcp-subtle">
                          {new Date(
                            Number(session.updated_at_ms || session.created_at_ms || 0)
                          ).toLocaleString()}
                        </span>
                      </div>
                    </button>
                  );
                })
              ) : (
                <EmptyState text="No workflow sessions have been saved yet." />
              )}
            </div>
          </PanelCard>
        }
        aside={
          <PanelCard
            title="Selected session"
            subtitle="This is the durable stored record, including provenance and draft linkage when available."
          >
            {selectedSession ? (
              <div className="grid gap-3">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge tone={selectedSession.source_kind === "imported_bundle" ? "warn" : "info"}>
                    {safeString(selectedSession.source_kind || "planner")}
                  </Badge>
                  <Badge tone="ghost">{safeString(selectedSession.session_id)}</Badge>
                </div>
                <div className="grid gap-2 text-sm">
                  <div>
                    <div className="tcp-subtle text-xs">Title</div>
                    <div>{safeString(selectedSession.title)}</div>
                  </div>
                  <div>
                    <div className="tcp-subtle text-xs">Project</div>
                    <div>{safeString(selectedSession.project_slug)}</div>
                  </div>
                  <div>
                    <div className="tcp-subtle text-xs">Bundle digest</div>
                    <div>{safeString(selectedSession.source_bundle_digest || "—")}</div>
                  </div>
                  <div>
                    <div className="tcp-subtle text-xs">Current plan id</div>
                    <div>{safeString(selectedSession.current_plan_id || "—")}</div>
                  </div>
                  <div>
                    <div className="tcp-subtle text-xs">Goal</div>
                    <div className="whitespace-pre-wrap">
                      {safeString(selectedSession.goal || "—")}
                    </div>
                  </div>
                  <div>
                    <div className="tcp-subtle text-xs">Import validation</div>
                    <pre className="max-h-40 overflow-auto rounded-xl bg-black/20 p-3 text-xs text-white/75">
                      {formatJson(selectedSession.import_validation || {})}
                    </pre>
                  </div>
                </div>
                <div className="flex flex-wrap gap-2">
                  <button type="button" className="tcp-btn-primary" onClick={openSelectedSession}>
                    Open in planner
                  </button>
                  <button
                    type="button"
                    className="tcp-btn-secondary"
                    onClick={() => copySessionId(selectedSession.session_id)}
                  >
                    Copy session id
                  </button>
                </div>
                {selectedSession.import_transform_log?.length ? (
                  <div className="grid gap-2">
                    <div className="tcp-subtle text-xs">Import transform log</div>
                    <pre className="max-h-40 overflow-auto rounded-xl bg-black/20 p-3 text-xs text-white/75">
                      {selectedSession.import_transform_log.join("\n")}
                    </pre>
                  </div>
                ) : null}
              </div>
            ) : (
              <EmptyState text="Select a session to inspect its provenance." />
            )}
          </PanelCard>
        }
      />
    </AnimatedPage>
  );
}
