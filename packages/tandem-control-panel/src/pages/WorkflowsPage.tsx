import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";
import { AnimatedPage, Badge, PageHeader, PanelCard, SplitView } from "../ui/index";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

const WORKFLOW_IMPORT_HANDOFF_KEY = "tandem.workflow.importHandoff.v1";

function safeString(value: unknown) {
  return String(value || "").trim();
}

import { LazyJson } from "../features/automations/LazyJson";

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

function payloadPath(payload: any, fallback = "") {
  const direct = safeString(payload?.absPath || payload?.abs_path || payload?.url);
  if (direct) return direct;
  const root = safeString(payload?.root);
  const path = safeString(payload?.path);
  if (root && path) return `${root.replace(/\/+$/, "")}/${path.replace(/^\/+/, "")}`;
  return safeString(path || fallback);
}

function uploadWorkflowAsset(file: File, dir: string) {
  return new Promise<any>((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open("POST", `/api/files/upload?dir=${encodeURIComponent(dir)}`);
    xhr.withCredentials = true;
    xhr.responseType = "json";
    xhr.setRequestHeader("x-file-name", encodeURIComponent(file.name));
    xhr.onerror = () => reject(new Error(`Upload failed: ${file.name}`));
    xhr.onload = () => {
      const payload = xhr.response || {};
      if (xhr.status < 200 || xhr.status >= 300 || payload?.ok === false) {
        reject(new Error(String(payload?.error || `Upload failed (${xhr.status})`)));
        return;
      }
      resolve(payload);
    };
    xhr.send(file);
  });
}

function engineDownloadUrl(path: unknown) {
  const value = safeString(path);
  if (!value) return "";
  if (value.startsWith("/api/engine/")) return value;
  if (value.startsWith("/")) return `/api/engine${value}`;
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
  const [packPath, setPackPath] = useState("");
  const [packFileName, setPackFileName] = useState("");
  const [packPreviewResult, setPackPreviewResult] = useState<any>(null);
  const [coverImagePath, setCoverImagePath] = useState("");
  const [coverImagePreview, setCoverImagePreview] = useState("");
  const [exportTitle, setExportTitle] = useState("");
  const [exportName, setExportName] = useState("");
  const [exportVersion, setExportVersion] = useState("0.1.0");
  const [exportDescription, setExportDescription] = useState("");
  const [exportResult, setExportResult] = useState<any>(null);

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

  const selectedSessionQuery = useQuery({
    queryKey: ["workflow-center", "session", selectedSessionId],
    enabled: !!selectedSessionId,
    queryFn: () =>
      client.workflowPlannerSessions.get(selectedSessionId).catch(() => ({ session: null })),
  });

  const selectedSession = selectedSessionQuery.data?.session || null;

  useEffect(() => {
    if (!selectedSession) {
      setExportTitle("");
      setExportName("");
      setExportDescription("");
      setCoverImagePath("");
      setCoverImagePreview("");
      setExportResult(null);
      return;
    }
    setExportTitle(safeString(selectedSession.title));
    setExportName(
      safeString(selectedSession.title)
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-|-$/g, "")
    );
    setExportDescription(safeString(selectedSession.goal));
    setCoverImagePath("");
    setCoverImagePreview("");
    setExportResult(null);
  }, [selectedSession?.session_id]);

  const handlePackFile = useCallback(
    async (file: File | null) => {
      if (!file) return;
      setImportError("");
      try {
        const payload = await uploadWorkflowAsset(file, "uploads/workflow-pack-imports");
        const path = payloadPath(payload, file.name);
        setPackPath(path);
        setPackFileName(file.name);
        setPackPreviewResult(null);
        toast("ok", "Workflow pack uploaded. Preview it before installing.");
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        setImportError(message);
        toast("err", message);
      }
    },
    [toast]
  );

  const handleCoverFile = useCallback(
    async (file: File | null) => {
      if (!file) return;
      try {
        const payload = await uploadWorkflowAsset(file, "uploads/workflow-pack-covers");
        setCoverImagePath(payloadPath(payload, file.name));
        setCoverImagePreview(URL.createObjectURL(file));
        toast("ok", "Cover image added to export.");
      } catch (error) {
        toast("err", error instanceof Error ? error.message : String(error));
      }
    },
    [toast]
  );

  const packPreviewMutation = useMutation({
    mutationFn: async () => {
      setImportError("");
      return client.workflowPlans.importPackPreview({
        path: packPath,
        creatorId,
        creator_id: creatorId,
        projectSlug,
        project_slug: projectSlug,
        title: safeString(title) || undefined,
      });
    },
    onSuccess: (payload) => {
      setPackPreviewResult(payload);
      toast("ok", "Workflow pack preview generated.");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setImportError(message);
      toast("err", message);
    },
  });

  const packImportMutation = useMutation({
    mutationFn: async () => {
      setImportError("");
      return client.workflowPlans.importPack({
        path: packPath,
        creatorId,
        creator_id: creatorId,
        projectSlug,
        project_slug: projectSlug,
        title: safeString(title) || undefined,
      });
    },
    onSuccess: async (payload) => {
      const session = (payload?.sessions?.[0] || payload?.session) as any;
      if (session?.session_id) {
        try {
          localStorage.setItem(
            WORKFLOW_IMPORT_HANDOFF_KEY,
            JSON.stringify({
              session_id: session.session_id,
              title: session.title,
              project_slug: session.project_slug,
              source_kind: session.source_kind || "workflow_pack",
              source_bundle_digest: session.source_bundle_digest || null,
              current_plan_id: session.current_plan_id || null,
            })
          );
        } catch {
          // Ignore storage failures.
        }
        setSelectedSessionId(session.session_id);
      }
      setPackPreviewResult(payload);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["workflow-center"] }),
        queryClient.invalidateQueries({ queryKey: ["intent-planner"] }),
      ]);
      toast("ok", "Workflow pack installed.");
      navigate("planner");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setImportError(message);
      toast("err", message);
    },
  });

  const exportPackMutation = useMutation({
    mutationFn: async () => {
      if (!selectedSessionId) throw new Error("Select a workflow session to export.");
      return client.workflowPlans.exportPack({
        sessionId: selectedSessionId,
        session_id: selectedSessionId,
        name: safeString(exportName) || undefined,
        title: safeString(exportTitle || selectedSession?.title) || undefined,
        version: safeString(exportVersion) || undefined,
        description: safeString(exportDescription) || undefined,
        creatorId,
        creator_id: creatorId,
        coverImagePath: safeString(coverImagePath) || undefined,
        cover_image_path: safeString(coverImagePath) || undefined,
      });
    },
    onSuccess: (payload) => {
      setExportResult(payload);
      toast("ok", "Workflow pack exported.");
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

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
        title="Workflow packs, sessions, and provenance"
        subtitle="Import marketplace-ready workflow packs, export your planner sessions, and keep JSON bundles available for advanced debugging."
        badges={
          <>
            <Badge tone="info">{sessions.length} sessions</Badge>
            <Badge tone="warn">
              {
                sessions.filter((session: any) =>
                  ["imported_bundle", "workflow_pack"].includes(session?.source_kind)
                ).length
              }{" "}
              imports
            </Badge>
          </>
        }
      />

      <PanelCard
        title="Import workflow pack"
        subtitle="Upload a .zip pack, preview its manifest and workflow contents, then install it into your local workflow library."
      >
        <div className="grid gap-3">
          <div className="grid gap-3 md:grid-cols-3">
            <label className="grid gap-2 text-sm md:col-span-3">
              <span className="tcp-subtle">Workflow pack ZIP</span>
              <input
                className="tcp-input"
                type="file"
                accept=".zip,application/zip"
                onChange={(event) => handlePackFile(event.currentTarget.files?.[0] || null)}
              />
              {packPath ? (
                <span className="text-xs text-white/70">
                  {packFileName || "Uploaded pack"} · {packPath}
                </span>
              ) : null}
            </label>
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
              disabled={!packPath || packPreviewMutation.isPending}
              onClick={() => packPreviewMutation.mutate()}
            >
              Preview pack
            </button>
            <button
              type="button"
              className="tcp-btn-primary"
              disabled={!packPath || packImportMutation.isPending}
              onClick={() => packImportMutation.mutate()}
            >
              Install and open planner
            </button>
          </div>
          {importError ? <div className="text-sm text-red-300">{importError}</div> : null}
          {packPreviewResult ? (
            <div className="grid gap-3 rounded-2xl border border-white/10 bg-black/20 p-4">
              <div className="flex flex-wrap items-start gap-4">
                {packPreviewResult?.cover_image_data_url ? (
                  <img
                    alt=""
                    className="h-24 w-40 border border-white/10 object-cover"
                    src={packPreviewResult.cover_image_data_url}
                  />
                ) : null}
                <div className="grid gap-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge tone={packPreviewResult?.persisted ? "ok" : "info"}>
                      {packPreviewResult?.persisted ? "installed" : "preview only"}
                    </Badge>
                    <Badge tone="ghost">
                      {safeString(packPreviewResult?.pack?.name || "workflow pack")}
                    </Badge>
                    <Badge tone="ghost">
                      {safeString(packPreviewResult?.pack?.version || "version")}
                    </Badge>
                    <Badge tone="info">
                      {toArray(packPreviewResult?.workflows, "workflows").length} workflows
                    </Badge>
                  </div>
                  <div className="text-sm text-white/75">
                    {safeString(
                      packPreviewResult?.manifest?.marketplace?.listing?.display_name ||
                        packPreviewResult?.manifest?.name
                    )}
                  </div>
                  <div className="flex flex-wrap gap-2 text-xs">
                    {toArray(packPreviewResult?.manifest?.capabilities?.required, "required").map(
                      (capability: any) => (
                        <span
                          key={String(capability)}
                          className="rounded border border-white/10 px-2 py-1 text-white/70"
                        >
                          {String(capability)}
                        </span>
                      )
                    )}
                  </div>
                </div>
              </div>
              <LazyJson
                value={
                  packPreviewResult?.workflows?.[0]?.plan_package_validation ||
                  packPreviewResult?.workflows?.[0]?.import_validation ||
                  packPreviewResult
                }
                label="Show validation details"
                preClassName="max-h-64 overflow-auto rounded-xl bg-black/30 p-3 text-xs text-white/80"
              />
            </div>
          ) : null}
          <details className="rounded-2xl border border-white/10 bg-black/10 p-3">
            <summary className="cursor-pointer text-sm font-semibold text-white">
              Advanced JSON bundle import
            </summary>
            <div className="mt-3 grid gap-3">
              <label className="grid gap-2 text-sm">
                <span className="tcp-subtle">Bundle JSON</span>
                <textarea
                  className="min-h-40 w-full rounded-xl border border-white/10 bg-black/20 p-3 font-mono text-xs text-white outline-none"
                  placeholder='{"plan_id":"...","mission":{...},"routine_graph":[...]}'
                  value={bundleText}
                  onChange={(event) => setBundleText(event.target.value)}
                />
              </label>
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  className="tcp-btn-secondary"
                  onClick={() => previewMutation.mutate()}
                >
                  Preview JSON
                </button>
                <button
                  type="button"
                  className="tcp-btn-secondary"
                  onClick={() => importMutation.mutate()}
                >
                  Import JSON
                </button>
              </div>
              {previewResult ? (
                <LazyJson
                  value={
                    previewResult?.plan_package_validation ||
                    previewResult?.import_validation ||
                    previewResult
                  }
                  label="Show JSON import validation"
                  preClassName="max-h-64 overflow-auto rounded-xl bg-black/30 p-3 text-xs text-white/80"
                />
              ) : null}
            </div>
          </details>
        </div>
      </PanelCard>

      <PanelCard
        title="Export workflow pack"
        subtitle={
          selectedSession
            ? "Turn this planner session into a marketplace-ready .zip pack with an optional cover image."
            : "Select a planner session below before exporting a workflow pack."
        }
      >
        <div className="grid gap-3 md:grid-cols-4">
          <label className="grid gap-2 text-sm md:col-span-2">
            <span className="tcp-subtle">Pack title</span>
            <input
              className="tcp-input"
              disabled={!selectedSession}
              placeholder="Select a workflow session"
              value={exportTitle}
              onChange={(event) => setExportTitle(event.target.value)}
            />
          </label>
          <label className="grid gap-2 text-sm">
            <span className="tcp-subtle">Pack slug</span>
            <input
              className="tcp-input"
              disabled={!selectedSession}
              placeholder="workflow-pack-slug"
              value={exportName}
              onChange={(event) => setExportName(event.target.value)}
            />
          </label>
          <label className="grid gap-2 text-sm">
            <span className="tcp-subtle">Version</span>
            <input
              className="tcp-input"
              disabled={!selectedSession}
              value={exportVersion}
              onChange={(event) => setExportVersion(event.target.value)}
            />
          </label>
          <label className="grid gap-2 text-sm md:col-span-3">
            <span className="tcp-subtle">Description</span>
            <input
              className="tcp-input"
              disabled={!selectedSession}
              placeholder="Short marketplace description"
              value={exportDescription}
              onChange={(event) => setExportDescription(event.target.value)}
            />
          </label>
          <label className="grid gap-2 text-sm">
            <span className="tcp-subtle">Cover image</span>
            <input
              className="tcp-input"
              type="file"
              disabled={!selectedSession}
              accept="image/png,image/jpeg,image/webp"
              onChange={(event) => handleCoverFile(event.currentTarget.files?.[0] || null)}
            />
          </label>
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-3">
          {coverImagePreview ? (
            <img
              alt=""
              className="h-20 w-32 border border-white/10 object-cover"
              src={coverImagePreview}
            />
          ) : null}
          <button
            type="button"
            className="tcp-btn-primary"
            disabled={!selectedSession || exportPackMutation.isPending}
            onClick={() => exportPackMutation.mutate()}
          >
            Export workflow pack
          </button>
        </div>
        {exportResult ? (
          <div className="mt-3 grid gap-2 rounded-2xl border border-emerald-400/30 bg-emerald-950/20 p-3 text-sm">
            <div className="font-semibold">Marketplace-ready workflow pack</div>
            <div className="break-all text-white/75">
              {safeString(exportResult?.exported?.path)}
            </div>
            <div className="text-xs text-white/60">
              SHA-256 {safeString(exportResult?.exported?.sha256)}
            </div>
            <div className="flex flex-wrap gap-2">
              {exportResult?.exported?.download_url ? (
                <a
                  className="tcp-btn-primary"
                  href={engineDownloadUrl(exportResult.exported.download_url)}
                >
                  Download ZIP
                </a>
              ) : null}
              <Badge tone="ok">tandempack.yaml</Badge>
              <Badge tone="ok">workflow plan bundle</Badge>
              {exportResult?.pack?.cover_image ? (
                <Badge tone="ok">cover image</Badge>
              ) : (
                <Badge tone="ghost">no cover image</Badge>
              )}
              <Badge tone="info">Upload as Workflow in marketplace</Badge>
            </div>
            <LazyJson
              value={exportResult?.manifest || exportResult}
              label="Show manifest"
              preClassName="max-h-64 overflow-auto rounded-xl bg-black/30 p-3 text-xs text-white/80"
            />
          </div>
        ) : null}
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
                    <LazyJson
                      value={selectedSession.import_validation || {}}
                      label="Show import validation"
                      preClassName="max-h-40 overflow-auto rounded-xl bg-black/20 p-3 text-xs text-white/75"
                    />
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
