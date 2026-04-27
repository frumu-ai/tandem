import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { renderMarkdownSafe } from "../lib/markdown";
import { useCapabilities } from "../features/system/queries.ts";
import { KnowledgebaseUploadPanel } from "../features/knowledgebase/KnowledgebaseUploadPanel";
import { ConfirmDialog, PromptDialog } from "../components/ControlPanelDialogs";
import { AnimatedPage, PanelCard, Toolbar, Badge } from "../ui/index.tsx";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";
import {
  consumeFilesExplorerHandoff,
  normalizeManagedFilesExplorerPath,
  parentManagedFilesExplorerDir,
} from "../features/files/explorerHandoff";

const EXPLORER_BUCKETS = ["uploads", "artifacts", "exports"];
const TEXT_PREVIEW_KINDS = new Set(["text", "markdown", "json", "yaml"]);
const FILE_PAGE_SIZE_OPTIONS = [10, 25, 50, 100];
const DEFAULT_FILE_PAGE_SIZE = 25;
type FileSurface = "workspace" | "managed";

type FileRow = {
  name: string;
  path: string;
  size?: number;
  updatedAt?: number;
  mime?: string;
  previewKind?: string;
  downloadUrl?: string;
};

function formatBytes(bytes: number) {
  const value = Number(bytes || 0);
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

function formatDateTime(ms: number) {
  const value = Number(ms || 0);
  if (!value) return "n/a";
  return new Date(value).toLocaleString();
}

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function pathDepth(path: string) {
  const clean = String(path || "").trim();
  if (!clean) return 0;
  return clean.split("/").filter(Boolean).length;
}

function normalizeWorkspaceExplorerPath(raw: string, allowEmpty = true) {
  const text = String(raw || "")
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\/+/, "")
    .replace(/\/+$/, "");
  if (!text) return allowEmpty ? "" : null;
  if (text.includes("\0")) return null;
  const parts = text.split("/").filter(Boolean);
  if (!parts.length) return allowEmpty ? "" : null;
  if (parts.some((part) => part === "." || part === "..")) return null;
  return parts.join("/");
}

function clampPage(page: number, totalPages: number) {
  if (!Number.isFinite(page) || page < 1) return 1;
  if (!Number.isFinite(totalPages) || totalPages < 1) return 1;
  return Math.min(page, totalPages);
}

function formatPageWindow(page: number, pageSize: number, total: number) {
  if (!total) return "0 of 0";
  const safePage = clampPage(page, Math.max(1, Math.ceil(total / Math.max(1, pageSize))));
  const safeSize = Math.max(1, pageSize);
  const start = (safePage - 1) * safeSize + 1;
  const end = Math.min(total, safePage * safeSize);
  return `${start}-${end} of ${total}`;
}

export function FilesPage({ api, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const capabilities = useCapabilities();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const uploadInputRef = useRef<HTMLInputElement | null>(null);
  const uploadFolderInputRef = useRef<HTMLInputElement | null>(null);
  const [fileSurface, setFileSurface] = useState<FileSurface>("managed");
  const [filesPanelCollapsed, setFilesPanelCollapsed] = useState(false);
  const [dir, setDir] = useState("");
  const [selectedPath, setSelectedPath] = useState("");
  const [createDirectoryDialog, setCreateDirectoryDialog] = useState<{
    baseDir: string;
    value: string;
  } | null>(null);
  const [uploadRows, setUploadRows] = useState<
    Array<{ id: string; name: string; progress: number; error: string }>
  >([]);
  const [filePage, setFilePage] = useState(1);
  const [filePageSize, setFilePageSize] = useState(DEFAULT_FILE_PAGE_SIZE);
  const [selectedFilePaths, setSelectedFilePaths] = useState<string[]>([]);
  const [deleteFilesConfirm, setDeleteFilesConfirm] = useState<{
    files: Array<{ path: string; name: string }>;
  } | null>(null);

  useEffect(() => {
    const handoff = consumeFilesExplorerHandoff();
    if (!handoff) return;
    setFileSurface("managed");
    if (handoff.dir) setDir(handoff.dir);
    else if (handoff.path) setDir(parentManagedFilesExplorerDir(handoff.path));
    setSelectedPath(handoff.path || handoff.dir || "");
  }, []);

  const workspaceFilesAvailable =
    capabilities.data?.workspace_files_available === true ||
    capabilities.data?.hosted_managed === true;

  useEffect(() => {
    if (!workspaceFilesAvailable) return;
    if (fileSurface !== "managed" || dir || selectedPath) return;
    setFileSurface("workspace");
  }, [workspaceFilesAvailable, fileSurface, dir, selectedPath]);

  const isWorkspaceMode = fileSurface === "workspace";
  const filesApiBase = isWorkspaceMode ? "/api/workspace/files" : "/api/files";

  const filesQuery = useQuery({
    queryKey: ["files", fileSurface, dir],
    queryFn: async () =>
      api(`${filesApiBase}/list?dir=${encodeURIComponent(dir)}`).catch(() => ({
        dir,
        parent: null,
        directories: [],
        files: [],
      })),
    refetchInterval: 15000,
  });

  const rootDir = String(filesQuery.data?.dir || "").trim();
  const parentDir = String(filesQuery.data?.parent || "").trim();
  const directories = toArray(filesQuery.data, "directories");
  const files = toArray(filesQuery.data, "files") as FileRow[];

  useEffect(() => {
    const availablePaths = new Set(
      files.map((file) => String(file.path || "").trim()).filter(Boolean)
    );
    setSelectedFilePaths((current) => current.filter((path) => availablePaths.has(path)));
  }, [files]);

  useEffect(() => {
    setFilePage(1);
  }, [dir]);

  const selectedFileSet = useMemo(() => new Set(selectedFilePaths), [selectedFilePaths]);
  const selectedFiles = useMemo(
    () => files.filter((file) => selectedFileSet.has(String(file.path || "").trim())),
    [files, selectedFileSet]
  );
  const selectedFileCount = selectedFiles.length;
  const filePageCount = Math.max(1, Math.ceil(files.length / Math.max(1, filePageSize)));
  const safeFilePage = clampPage(filePage, filePageCount);
  const filePageStart = (safeFilePage - 1) * Math.max(1, filePageSize);
  const pagedFiles = files.slice(filePageStart, filePageStart + Math.max(1, filePageSize));
  const filePageLabel = formatPageWindow(safeFilePage, filePageSize, files.length);

  useEffect(() => {
    if (filePage !== safeFilePage) setFilePage(safeFilePage);
  }, [filePage, safeFilePage]);

  const previewCandidate = useMemo(() => {
    return (
      files.find((entry: any) => String(entry?.path || "") === selectedPath) ||
      directories.find((entry: any) => String(entry?.path || "") === selectedPath) ||
      null
    );
  }, [filesQuery.data, selectedPath]);

  const selectedDirectory =
    (!!previewCandidate && String(previewCandidate?.previewKind || "") === "directory") ||
    (!!rootDir && selectedPath === rootDir);
  const selectedFile =
    !!previewCandidate && !selectedDirectory ? (previewCandidate as FileRow) : null;
  const selectedMime = String(selectedFile?.mime || "").trim();
  const selectedPreviewKind = String(selectedFile?.previewKind || "").trim();
  const selectedDownloadUrl =
    selectedFile?.downloadUrl ||
    `${filesApiBase}/download?path=${encodeURIComponent(selectedPath)}`;
  const selectedTextPreview = useQuery({
    queryKey: ["files", fileSurface, "read", selectedPath],
    enabled: !!selectedFile && TEXT_PREVIEW_KINDS.has(selectedPreviewKind),
    queryFn: async () =>
      api(`${filesApiBase}/read?path=${encodeURIComponent(selectedPath)}`).catch((error) => ({
        ok: false,
        previewable: false,
        reason: "unavailable",
        error: error instanceof Error ? error.message : String(error),
      })),
  });
  const currentCount = directories.length + files.length;
  const currentDepth = pathDepth(rootDir);
  const selectedPreviewable =
    !!selectedFile &&
    TEXT_PREVIEW_KINDS.has(selectedPreviewKind) &&
    selectedTextPreview.data?.previewable !== false;
  const selectedPreviewLoading = selectedTextPreview.isFetching && selectedPreviewable;

  useEffect(() => {
    if (rootRef.current) renderIcons(rootRef.current);
  }, [
    currentCount,
    directories.length,
    files.length,
    filePage,
    filePageSize,
    pagedFiles.length,
    selectedFileCount,
    selectedPath,
    selectedPreviewKind,
    selectedDirectory,
    uploadRows.length,
    rootDir,
    fileSurface,
    filesPanelCollapsed,
    selectedTextPreview.data?.previewable,
  ]);

  const uploadOne = useMutation({
    mutationFn: ({
      file,
      targetDir,
      surface,
      relativePath,
    }: {
      file: File;
      targetDir: string;
      surface: FileSurface;
      relativePath?: string;
    }) =>
      new Promise<any>((resolve, reject) => {
        const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
        setUploadRows((prev) => [...prev, { id, name: file.name, progress: 0, error: "" }]);

        const xhr = new XMLHttpRequest();
        const endpoint =
          surface === "workspace" ? "/api/workspace/files/upload" : "/api/files/upload";
        xhr.open("POST", `${endpoint}?dir=${encodeURIComponent(targetDir)}`);
        xhr.withCredentials = true;
        xhr.responseType = "json";
        xhr.setRequestHeader("x-file-name", encodeURIComponent(file.name));
        if (surface === "workspace" && relativePath) {
          xhr.setRequestHeader("x-relative-path", encodeURIComponent(relativePath));
        }

        xhr.upload.onprogress = (event) => {
          if (!event.lengthComputable) return;
          const pct = (event.loaded / event.total) * 100;
          setUploadRows((prev) =>
            prev.map((row) => (row.id === id ? { ...row, progress: pct } : row))
          );
        };

        xhr.onerror = () => {
          setUploadRows((prev) =>
            prev.map((row) => (row.id === id ? { ...row, error: "Network error" } : row))
          );
          window.setTimeout(() => {
            setUploadRows((prev) => prev.filter((row) => row.id !== id));
          }, 1200);
          reject(new Error(`Upload failed: ${file.name}`));
        };

        xhr.onload = () => {
          const payload = xhr.response || {};
          if (xhr.status < 200 || xhr.status >= 300 || payload?.ok === false) {
            const message = String(payload?.error || `Upload failed (${xhr.status})`);
            setUploadRows((prev) =>
              prev.map((row) => (row.id === id ? { ...row, error: message } : row))
            );
            window.setTimeout(() => {
              setUploadRows((prev) => prev.filter((row) => row.id !== id));
            }, 1600);
            reject(new Error(message));
            return;
          }

          setUploadRows((prev) => prev.filter((row) => row.id !== id));
          resolve(payload);
        };

        xhr.send(file);
      }),
    onSuccess: async (payload: any, vars) => {
      const nextPath = String(payload?.path || "").trim();
      if (nextPath) {
        const nextDir =
          String(vars.targetDir || "").trim() || parentManagedFilesExplorerDir(nextPath);
        if (vars.surface === "managed" && !dir && nextDir === "uploads") setDir("uploads");
        setSelectedPath(nextPath);
      }
      await queryClient.invalidateQueries({ queryKey: ["files"] });
      toast(
        "ok",
        `Uploaded ${String(vars.file?.name || "file")} into ${
          vars.targetDir || (vars.surface === "workspace" ? "Workspace" : "uploads")
        }.`
      );
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const createDirectory = useMutation({
    mutationFn: async (path: string) =>
      api(`${filesApiBase}/mkdir`, {
        method: "POST",
        body: JSON.stringify({ path }),
      }),
    onSuccess: async (payload: any) => {
      const nextPath = String(payload?.path || "").trim();
      if (nextPath) {
        setDir(nextPath);
        setSelectedPath(nextPath);
      }
      await queryClient.invalidateQueries({ queryKey: ["files"] });
      toast("ok", `Created folder ${nextPath || "folder"}.`);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const uploadFiles = async (fileList: FileList | null) => {
    const filesToUpload = [...(fileList || [])];
    if (!filesToUpload.length) return;
    const targetDir = isWorkspaceMode ? dir : dir || "uploads";
    for (const file of filesToUpload) {
      const relativePath = isWorkspaceMode
        ? String((file as File & { webkitRelativePath?: string }).webkitRelativePath || file.name)
        : undefined;
      await uploadOne
        .mutateAsync({ file, targetDir, surface: fileSurface, relativePath })
        .catch(() => undefined);
    }
    if (!isWorkspaceMode && !dir) setDir("uploads");
  };

  const handleCreateDirectory = () => {
    const baseDir = String(dir || "").trim();
    if (!isWorkspaceMode && !baseDir) {
      toast("warn", "Choose a bucket before creating folders.");
      return;
    }
    setCreateDirectoryDialog({
      baseDir,
      value: isWorkspaceMode ? "new-folder" : "knowledgebooks/new-collection",
    });
  };

  const submitCreateDirectory = async () => {
    const dialog = createDirectoryDialog;
    if (!dialog) return;
    const cleaned = String(dialog.value || "")
      .trim()
      .replace(/^\/+|\/+$/g, "")
      .replace(/\/{2,}/g, "/");
    if (!cleaned) {
      toast("warn", "Enter a folder name.");
      return;
    }
    setCreateDirectoryDialog(null);
    await createDirectory.mutateAsync(dialog.baseDir ? `${dialog.baseDir}/${cleaned}` : cleaned);
  };

  const openDirectory = (path: string) => {
    const next = isWorkspaceMode
      ? normalizeWorkspaceExplorerPath(path, true)
      : normalizeManagedFilesExplorerPath(path);
    if (!next) return;
    setDir(next);
    setSelectedPath(next);
  };

  const openParent = () => {
    if (!parentDir) {
      setDir("");
      setSelectedPath("");
      return;
    }
    setDir(parentDir);
    setSelectedPath(parentDir);
  };

  const openRoot = () => {
    setDir("");
    setSelectedPath("");
  };

  const toggleFileSelection = (path: string) => {
    const cleaned = String(path || "").trim();
    if (!cleaned) return;
    setSelectedFilePaths((current) =>
      current.includes(cleaned)
        ? current.filter((entry) => entry !== cleaned)
        : [...current, cleaned]
    );
  };

  const selectAllFiles = () => {
    const paths = files.map((file) => String(file.path || "").trim()).filter(Boolean);
    setSelectedFilePaths(Array.from(new Set(paths)));
  };

  const clearSelectedFiles = () => {
    setSelectedFilePaths([]);
  };

  const openDeleteSelectedFiles = () => {
    if (!selectedFiles.length) {
      toast("warn", "Select one or more files first.");
      return;
    }
    setDeleteFilesConfirm({
      files: selectedFiles.map((file) => ({
        path: String(file.path || "").trim(),
        name: String(file.name || file.path || "").trim(),
      })),
    });
  };

  const confirmDeleteSelectedFiles = async () => {
    const targets = deleteFilesConfirm?.files || [];
    if (!targets.length) {
      toast("warn", "Select one or more files first.");
      return;
    }

    const remaining = new Set(selectedFilePaths);
    let okCount = 0;
    let failCount = 0;

    for (const target of targets) {
      try {
        await api(`${filesApiBase}/delete`, {
          method: "POST",
          body: JSON.stringify({ path: target.path }),
        });
        okCount += 1;
        remaining.delete(target.path);
      } catch {
        failCount += 1;
      }
    }

    setSelectedFilePaths(Array.from(remaining));
    setSelectedPath((current) =>
      targets.some((target) => target.path === current) ? "" : current
    );
    setDeleteFilesConfirm(null);

    await queryClient.invalidateQueries({ queryKey: ["files"] });
    if (okCount && failCount) {
      toast("warn", `Deleted ${okCount} file(s); ${failCount} failed.`);
    } else if (okCount) {
      toast("ok", `Deleted ${okCount} file(s).`);
    } else {
      toast("err", `All ${failCount} selected file deletions failed.`);
    }
  };

  const switchFileSurface = (surface: FileSurface) => {
    if (surface === fileSurface) return;
    setFileSurface(surface);
    setDir("");
    setSelectedPath("");
    setSelectedFilePaths([]);
    setFilePage(1);
  };

  const currentLabel = isWorkspaceMode
    ? !rootDir
      ? "Workspace"
      : rootDir
    : !rootDir
      ? "Root"
      : rootDir;
  const currentPreviewReason = String(selectedTextPreview.data?.reason || "").trim();
  const selectedPreviewText = String(selectedTextPreview.data?.text || "").trim();
  const bucketCount = EXPLORER_BUCKETS.length;
  const defaultKnowledgebaseCollectionId = String(
    capabilities.data?.hosted_deployment_slug || capabilities.data?.hosted_hostname || ""
  ).trim();

  return (
    <AnimatedPage className="grid h-full min-h-0 gap-4">
      <KnowledgebaseUploadPanel
        api={api}
        toast={toast}
        defaultCollectionId={defaultKnowledgebaseCollectionId}
      />
      <PanelCard
        fullHeight
        title="Files"
        subtitle={
          isWorkspaceMode
            ? "Browse and edit the hosted workspace."
            : "Browse managed uploads, published artifacts, and exports."
        }
        actions={
          <Toolbar className="justify-start">
            <button
              type="button"
              className="tcp-btn"
              onClick={() => uploadInputRef.current?.click()}
              disabled={uploadOne.isPending}
            >
              <i data-lucide="upload"></i>
              Upload
            </button>
            {isWorkspaceMode ? (
              <button
                type="button"
                className="tcp-btn"
                onClick={() => uploadFolderInputRef.current?.click()}
                disabled={uploadOne.isPending}
              >
                <i data-lucide="folder-up"></i>
                Upload folder
              </button>
            ) : null}
            <button
              type="button"
              className="tcp-btn"
              onClick={handleCreateDirectory}
              disabled={createDirectory.isPending || (!isWorkspaceMode && !dir)}
              title={
                !isWorkspaceMode && !dir
                  ? "Choose a bucket first"
                  : "Create a folder inside the current path"
              }
            >
              <i data-lucide="folder-plus"></i>
              New folder
            </button>
            <button type="button" className="tcp-btn" onClick={() => void filesQuery.refetch()}>
              <i data-lucide="refresh-cw"></i>
              Refresh
            </button>
            <button type="button" className="tcp-btn" onClick={openParent} disabled={!rootDir}>
              <i data-lucide="corner-up-left"></i>
              Up
            </button>
            <button
              type="button"
              className="tcp-btn"
              onClick={() => setFilesPanelCollapsed((current) => !current)}
            >
              <i data-lucide={filesPanelCollapsed ? "chevron-down" : "chevron-up"}></i>
              {filesPanelCollapsed ? "Expand" : "Collapse"}
            </button>
          </Toolbar>
        }
      >
        <div
          ref={rootRef}
          className={
            filesPanelCollapsed
              ? "hidden"
              : "grid min-h-0 gap-4 xl:grid-cols-[280px_minmax(0,1fr)_360px]"
          }
        >
          <PanelCard
            fullHeight
            className="overflow-hidden"
            title="Locations"
            subtitle={isWorkspaceMode ? "Hosted workspace root." : "Managed file buckets."}
            actions={<Badge tone="ghost">{isWorkspaceMode ? "workspace" : bucketCount}</Badge>}
          >
            <div className="grid min-h-0 gap-3 p-4">
              <div className="grid grid-cols-2 gap-2">
                <button
                  type="button"
                  className={`tcp-btn h-9 justify-center px-2 text-xs ${
                    isWorkspaceMode ? "border-sky-500/40 bg-sky-950/20" : ""
                  }`.trim()}
                  onClick={() => switchFileSurface("workspace")}
                  disabled={!workspaceFilesAvailable}
                  title={
                    workspaceFilesAvailable
                      ? "Browse the workspace"
                      : "Workspace root is not configured"
                  }
                >
                  <i data-lucide="folder-code"></i>
                  Workspace
                </button>
                <button
                  type="button"
                  className={`tcp-btn h-9 justify-center px-2 text-xs ${
                    !isWorkspaceMode ? "border-sky-500/40 bg-sky-950/20" : ""
                  }`.trim()}
                  onClick={() => switchFileSurface("managed")}
                >
                  <i data-lucide="archive"></i>
                  Managed
                </button>
              </div>

              <div className="grid gap-1">
                <button
                  type="button"
                  className={`tcp-list-item w-full text-left ${!rootDir ? "border-sky-500/40 bg-sky-950/20" : ""}`}
                  onClick={openRoot}
                >
                  <i data-lucide="hard-drive"></i>
                  {isWorkspaceMode ? "Workspace root" : "Root"}
                </button>
                {!isWorkspaceMode
                  ? EXPLORER_BUCKETS.map((bucket) => {
                      const active = rootDir === bucket;
                      return (
                        <button
                          key={bucket}
                          type="button"
                          className={`tcp-list-item w-full text-left ${active ? "border-sky-500/40 bg-sky-950/20" : ""}`}
                          onClick={() => openDirectory(bucket)}
                        >
                          <i data-lucide="folder-open"></i>
                          <span className="flex min-w-0 flex-1 items-center justify-between gap-2">
                            <span className="truncate">{bucket}</span>
                            <span className="tcp-subtle text-[11px]">
                              {bucket === "uploads" ? "managed" : bucket}
                            </span>
                          </span>
                        </button>
                      );
                    })
                  : null}
              </div>

              <div className="rounded-xl border border-white/10 bg-black/20 p-3">
                <div className="tcp-subtle text-xs uppercase tracking-wide">Path</div>
                <div className="mt-2 flex flex-wrap gap-2">
                  <button type="button" className="tcp-btn h-7 px-2 text-xs" onClick={openRoot}>
                    {isWorkspaceMode ? "Workspace" : "Root"}
                  </button>
                  {rootDir
                    .split("/")
                    .filter(Boolean)
                    .reduce<Array<string>>((acc, segment) => {
                      const next = acc.length ? `${acc[acc.length - 1]}/${segment}` : segment;
                      acc.push(next);
                      return acc;
                    }, [])
                    .map((segment) => (
                      <button
                        key={segment}
                        type="button"
                        className={`tcp-btn h-7 px-2 text-xs ${segment === rootDir ? "border-sky-500/40 bg-sky-950/20" : ""}`.trim()}
                        onClick={() => openDirectory(segment)}
                      >
                        {segment.split("/").pop() || segment}
                      </button>
                    ))}
                </div>
              </div>

              {uploadRows.length ? (
                <div className="grid gap-2">
                  <div className="tcp-subtle text-xs uppercase tracking-wide">Uploads</div>
                  <div className="grid gap-2">
                    {uploadRows.map((row) => (
                      <div
                        key={row.id}
                        className="rounded-xl border border-white/10 bg-black/20 p-2 text-xs"
                      >
                        <div className="flex items-center justify-between gap-2">
                          <span className="truncate">{row.name}</span>
                          <span className="tcp-subtle">{Math.round(row.progress)}%</span>
                        </div>
                        <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-white/10">
                          <div
                            className={`h-full rounded-full ${row.error ? "bg-rose-400" : "bg-sky-400"}`}
                            style={{ width: `${Math.max(4, Math.min(100, row.progress || 0))}%` }}
                          ></div>
                        </div>
                        {row.error ? <div className="mt-1 text-rose-200">{row.error}</div> : null}
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}
            </div>
          </PanelCard>

          <PanelCard
            fullHeight
            className="overflow-hidden"
            title={currentLabel}
            subtitle={
              !isWorkspaceMode && currentDepth === 0
                ? "Select a bucket to browse its files."
                : `${directories.length} folder${directories.length === 1 ? "" : "s"} and ${files.length} file${files.length === 1 ? "" : "s"}`
            }
            actions={
              <div className="flex flex-wrap items-center justify-end gap-2">
                <span className="tcp-badge tcp-badge-ghost">{currentLabel}</span>
                {selectedPreviewKind ? <Badge tone="info">{selectedPreviewKind}</Badge> : null}
                {selectedDirectory ? <Badge tone="ghost">folder</Badge> : null}
              </div>
            }
          >
            <div className="flex min-h-0 flex-1 flex-col gap-3 p-4">
              <div className="tcp-toolbar">
                <span className="tcp-badge-info">
                  {directories.length} folder{directories.length === 1 ? "" : "s"}
                </span>
                <span className="tcp-badge-info">
                  {files.length} file{files.length === 1 ? "" : "s"}
                </span>
                <span className={dir ? "tcp-badge-ok" : "tcp-badge tcp-badge-ghost"}>
                  {dir ? "browsing" : "root"}
                </span>
              </div>

              <div className="grid min-h-0 gap-2 overflow-auto pr-1">
                {!isWorkspaceMode && currentDepth === 0 ? (
                  <EmptyState
                    text="Pick a bucket on the left, or upload a file to start browsing."
                    title="No folder selected"
                  />
                ) : directories.length || files.length ? (
                  <>
                    {directories.map((entry: any) => {
                      const path = String(entry?.path || "");
                      const active = path === selectedPath;
                      return (
                        <button
                          key={`dir-${path}`}
                          type="button"
                          className={`tcp-list-item w-full text-left ${active ? "border-sky-500/40 bg-sky-950/20" : ""}`}
                          onClick={() => openDirectory(path)}
                        >
                          <i data-lucide="folder-open"></i>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center justify-between gap-2">
                              <strong className="truncate">{String(entry?.name || path)}</strong>
                              <span className="tcp-subtle text-xs">folder</span>
                            </div>
                            <div className="mt-1 flex items-center justify-between gap-2 text-xs tcp-subtle">
                              <span className="truncate">{path}</span>
                              <span>{formatDateTime(Number(entry?.updatedAt || 0))}</span>
                            </div>
                          </div>
                        </button>
                      );
                    })}

                    <div className="grid gap-2 rounded-xl border border-white/10 bg-black/20 p-3">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="flex flex-wrap items-center gap-2">
                          <Badge tone="ghost">{filePageLabel}</Badge>
                          <Badge tone={selectedFileCount ? "info" : "ghost"}>
                            {selectedFileCount} selected
                          </Badge>
                        </div>
                        <div className="flex flex-wrap items-center gap-2">
                          <button
                            type="button"
                            className="tcp-btn h-8 px-3 text-xs"
                            onClick={selectAllFiles}
                            disabled={!files.length}
                          >
                            Select all
                          </button>
                          <button
                            type="button"
                            className="tcp-btn h-8 px-3 text-xs"
                            onClick={clearSelectedFiles}
                            disabled={!selectedFileCount}
                          >
                            Clear selection
                          </button>
                          <button
                            type="button"
                            className="tcp-btn h-8 px-3 text-xs border-rose-500/30 text-rose-100 hover:bg-rose-950/20 disabled:opacity-50"
                            onClick={openDeleteSelectedFiles}
                            disabled={!selectedFileCount}
                          >
                            <i data-lucide="trash-2"></i>
                            Delete selected
                          </button>
                          <label className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-slate-500">
                            <span>Per page</span>
                            <select
                              className="tcp-select h-8 min-w-[5.5rem] px-3 text-center text-sm font-semibold tabular-nums text-slate-100 [text-align-last:center]"
                              value={filePageSize}
                              onChange={(event) => {
                                setFilePageSize(
                                  Number(event.target.value) || DEFAULT_FILE_PAGE_SIZE
                                );
                                setFilePage(1);
                              }}
                            >
                              {FILE_PAGE_SIZE_OPTIONS.map((value) => (
                                <option key={value} value={value}>
                                  {value}
                                </option>
                              ))}
                            </select>
                          </label>
                          <button
                            type="button"
                            className="tcp-btn h-8 px-3 text-xs"
                            onClick={() =>
                              setFilePage((page) => clampPage(page - 1, filePageCount))
                            }
                            disabled={safeFilePage <= 1}
                          >
                            Prev
                          </button>
                          <button
                            type="button"
                            className="tcp-btn h-8 px-3 text-xs"
                            onClick={() =>
                              setFilePage((page) => clampPage(page + 1, filePageCount))
                            }
                            disabled={safeFilePage >= filePageCount}
                          >
                            Next
                          </button>
                        </div>
                      </div>

                      <div className="grid gap-2">
                        {pagedFiles.map((entry: FileRow) => {
                          const path = String(entry?.path || "");
                          const active = path === selectedPath;
                          const checked = selectedFileSet.has(path);
                          const kind = String(entry?.previewKind || "file");
                          return (
                            <motion.div key={`file-${path}`} layout>
                              <div className="flex items-start gap-3">
                                <label
                                  className={`mt-3 flex h-6 w-6 items-center justify-center rounded border ${
                                    path
                                      ? "border-white/15 bg-black/20 text-slate-200 cursor-pointer hover:border-sky-500/40"
                                      : "border-white/10 bg-black/10 text-slate-500"
                                  }`}
                                >
                                  <input
                                    type="checkbox"
                                    className="h-4 w-4 accent-sky-400"
                                    checked={checked}
                                    onChange={() => toggleFileSelection(path)}
                                    aria-label={`Select ${entry.name || path}`}
                                  />
                                </label>
                                <button
                                  type="button"
                                  className={`tcp-list-item min-w-0 flex-1 text-left ${
                                    active ? "border-sky-500/40 bg-sky-950/20" : ""
                                  }`}
                                  onClick={() =>
                                    setSelectedPath((current) => (current === path ? "" : path))
                                  }
                                >
                                  <i data-lucide="file-text"></i>
                                  <div className="min-w-0 flex-1">
                                    <div className="flex items-center justify-between gap-2">
                                      <strong className="truncate">{entry.name || path}</strong>
                                      <span className="tcp-subtle text-xs">{kind}</span>
                                    </div>
                                    <div className="mt-1 flex flex-wrap items-center gap-2 text-xs tcp-subtle">
                                      <span className="truncate">{path}</span>
                                      <span>{formatBytes(Number(entry?.size || 0))}</span>
                                      <span>{formatDateTime(Number(entry?.updatedAt || 0))}</span>
                                    </div>
                                  </div>
                                </button>
                              </div>
                            </motion.div>
                          );
                        })}
                      </div>
                    </div>
                  </>
                ) : (
                  <EmptyState text="This folder does not contain any files yet." />
                )}
              </div>
            </div>
          </PanelCard>

          <PanelCard
            fullHeight
            className="overflow-hidden"
            title="Preview"
            subtitle={
              selectedFile ? selectedFile.path : selectedDirectory ? selectedPath : "Select a file"
            }
            actions={
              selectedFile ? (
                <a
                  className="tcp-btn h-8 px-3 text-xs"
                  href={selectedDownloadUrl}
                  target="_blank"
                  rel="noreferrer"
                >
                  <i data-lucide="download"></i>
                  Download
                </a>
              ) : selectedDirectory ? (
                <button
                  type="button"
                  className="tcp-btn h-8 px-3 text-xs"
                  onClick={() => openDirectory(selectedPath)}
                >
                  <i data-lucide="folder-open"></i>
                  Open
                </button>
              ) : null
            }
          >
            <div className="flex min-h-0 flex-1 flex-col gap-3 p-4">
              {selectedFile ? (
                <>
                  <div className="rounded-xl border border-white/10 bg-black/20 p-3">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <div className="truncate text-sm font-semibold">{selectedFile.name}</div>
                        <div className="tcp-subtle mt-1 text-xs">{selectedFile.path}</div>
                      </div>
                      <div className="flex flex-col items-end gap-1">
                        <Badge tone="info">{selectedPreviewKind || "file"}</Badge>
                        <span className="tcp-subtle text-xs">
                          {selectedMime || "application/octet-stream"}
                        </span>
                      </div>
                    </div>
                    <div className="mt-3 grid grid-cols-2 gap-2 text-xs">
                      <div className="rounded-lg border border-white/10 bg-black/10 p-2">
                        <div className="tcp-subtle">Size</div>
                        <div className="mt-1 font-medium">
                          {formatBytes(Number(selectedFile.size || 0))}
                        </div>
                      </div>
                      <div className="rounded-lg border border-white/10 bg-black/10 p-2">
                        <div className="tcp-subtle">Updated</div>
                        <div className="mt-1 font-medium">
                          {formatDateTime(Number(selectedFile.updatedAt || 0))}
                        </div>
                      </div>
                    </div>
                  </div>

                  {selectedPreviewKind === "image" ? (
                    <div className="min-h-0 flex-1 overflow-auto rounded-xl border border-white/10 bg-black/20 p-3">
                      <img
                        src={selectedDownloadUrl}
                        alt={selectedFile.name}
                        className="max-h-full max-w-full rounded-lg object-contain"
                      />
                    </div>
                  ) : selectedPreviewKind === "pdf" ? (
                    <div className="min-h-0 flex-1 overflow-hidden rounded-xl border border-white/10 bg-black/20">
                      <iframe
                        title={selectedFile.name}
                        src={selectedDownloadUrl}
                        className="h-full w-full"
                      />
                    </div>
                  ) : selectedPreviewable ? (
                    <div className="min-h-0 flex-1 overflow-auto rounded-xl border border-white/10 bg-black/20 p-3">
                      {selectedPreviewLoading ? (
                        <div className="tcp-subtle text-sm">Loading preview...</div>
                      ) : selectedPreviewKind === "markdown" ? (
                        <div
                          className="tcp-markdown tcp-markdown-ai"
                          dangerouslySetInnerHTML={{
                            __html: renderMarkdownSafe(selectedPreviewText || " "),
                          }}
                        />
                      ) : (
                        <pre className="tcp-code whitespace-pre-wrap break-words">
                          {selectedPreviewText || " "}
                        </pre>
                      )}
                    </div>
                  ) : (
                    <div className="grid min-h-0 flex-1 gap-3">
                      <EmptyState
                        title="Preview unavailable"
                        text={
                          currentPreviewReason === "too_large"
                            ? `This file is larger than ${formatBytes(2 * 1024 * 1024)} for inline preview.`
                            : currentPreviewReason === "unavailable"
                              ? "The preview request failed or the file is no longer available."
                              : "This file type is not previewed inline."
                        }
                      />
                      <div className="rounded-xl border border-white/10 bg-black/20 p-3 text-xs">
                        <div className="tcp-subtle">Download URL</div>
                        <a className="mt-1 block truncate text-sky-200" href={selectedDownloadUrl}>
                          {selectedDownloadUrl}
                        </a>
                      </div>
                    </div>
                  )}
                </>
              ) : selectedDirectory ? (
                <div className="grid gap-3">
                  <div className="rounded-xl border border-white/10 bg-black/20 p-3">
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <div className="text-sm font-semibold">{selectedPath || "Folder"}</div>
                        <div className="tcp-subtle mt-1 text-xs">Folder preview</div>
                      </div>
                      <Badge tone="ghost">folder</Badge>
                    </div>
                    <div className="mt-3 grid grid-cols-2 gap-2 text-xs">
                      <div className="rounded-lg border border-white/10 bg-black/10 p-2">
                        <div className="tcp-subtle">Folders</div>
                        <div className="mt-1 font-medium">{directories.length}</div>
                      </div>
                      <div className="rounded-lg border border-white/10 bg-black/10 p-2">
                        <div className="tcp-subtle">Files</div>
                        <div className="mt-1 font-medium">{files.length}</div>
                      </div>
                    </div>
                  </div>
                  <div className="rounded-xl border border-white/10 bg-black/20 p-3 text-xs">
                    <div className="tcp-subtle">Folder path</div>
                    <div className="mt-1 break-all">{selectedPath}</div>
                  </div>
                </div>
              ) : (
                <EmptyState
                  title="No selection"
                  text="Pick a file to preview it here, or select a folder to see its details."
                />
              )}
            </div>
          </PanelCard>
        </div>
      </PanelCard>

      <input
        ref={uploadInputRef}
        type="file"
        className="hidden"
        multiple
        onChange={(event) => {
          void uploadFiles((event.target as HTMLInputElement).files);
          (event.target as HTMLInputElement).value = "";
        }}
      />
      <input
        ref={uploadFolderInputRef}
        type="file"
        className="hidden"
        multiple
        {...({ webkitdirectory: "", directory: "" } as any)}
        onChange={(event) => {
          void uploadFiles((event.target as HTMLInputElement).files);
          (event.target as HTMLInputElement).value = "";
        }}
      />

      <PromptDialog
        open={!!createDirectoryDialog}
        title="Create folder"
        message={
          <span>
            Create a new folder inside{" "}
            <strong>
              {createDirectoryDialog?.baseDir || (isWorkspaceMode ? "Workspace" : "current path")}
            </strong>
            .
          </span>
        }
        label="Folder path"
        value={createDirectoryDialog?.value || ""}
        placeholder={isWorkspaceMode ? "new-folder" : "knowledgebooks/new-collection"}
        confirmLabel="Create folder"
        confirmIcon="folder-plus"
        confirmDisabled={!String(createDirectoryDialog?.value || "").trim()}
        onCancel={() => setCreateDirectoryDialog(null)}
        onChange={(value) =>
          setCreateDirectoryDialog((current) => (current ? { ...current, value } : current))
        }
        onConfirm={() => void submitCreateDirectory()}
      />

      <ConfirmDialog
        open={!!deleteFilesConfirm}
        title="Delete selected files"
        message={
          <span>
            This will permanently remove{" "}
            <strong>{deleteFilesConfirm?.files.length || 0} selected file(s)</strong> from{" "}
            <strong>{currentLabel || "the current folder"}</strong>.
          </span>
        }
        confirmLabel="Delete selected"
        confirmIcon="trash-2"
        confirmTone="danger"
        widthClassName="w-[min(42rem,96vw)]"
        onCancel={() => setDeleteFilesConfirm(null)}
        onConfirm={() => void confirmDeleteSelectedFiles()}
      >
        {deleteFilesConfirm?.files.length ? (
          <div className="mt-3 grid gap-2 rounded-xl border border-white/10 bg-black/20 p-3 text-left text-xs">
            <div className="tcp-subtle uppercase tracking-wide">Selected files</div>
            <div className="max-h-40 overflow-auto">
              {deleteFilesConfirm.files.slice(0, 12).map((file) => (
                <div key={file.path} className="truncate py-1">
                  <span className="font-medium text-slate-100">{file.name || file.path}</span>
                  <span className="tcp-subtle"> · {file.path}</span>
                </div>
              ))}
              {deleteFilesConfirm.files.length > 12 ? (
                <div className="pt-1 text-slate-400">
                  +{deleteFilesConfirm.files.length - 12} more
                </div>
              ) : null}
            </div>
          </div>
        ) : null}
      </ConfirmDialog>
    </AnimatedPage>
  );
}
