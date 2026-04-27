import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { renderIcons } from "../../app/icons.js";
import { renderMarkdownSafe } from "../../lib/markdown";
import { ConfirmDialog } from "../../components/ControlPanelDialogs";
import { Badge, EmptyState, PanelCard, Toolbar } from "../../ui/index.tsx";

type KnowledgebaseCollection = {
  collection_id: string;
  document_count?: number;
  updated_at?: number;
};

type KnowledgebaseDocument = {
  doc_id?: string;
  id?: string;
  collection_id?: string;
  title?: string;
  filename?: string;
  file_name?: string;
  updated_at?: number;
  created_at?: number;
  content?: string;
  excerpt?: string;
  content_type?: string;
  path?: string;
  size_bytes?: number;
};

type KnowledgebaseDocumentSelection = {
  collection: string;
  slug: string;
  title: string;
  key: string;
};

type KnowledgebaseDocumentPage = {
  collection_id?: string | null;
  query?: string | null;
  total?: number;
  offset?: number;
  limit?: number;
  has_more?: boolean;
  documents?: KnowledgebaseDocument[];
};

type KnowledgebaseUploadResult = {
  docId: string;
  collectionId: string;
  title: string;
};

type UploadRow = {
  id: string;
  name: string;
  progress: number;
  status: "queued" | "uploading" | "done" | "error";
  error: string;
  result?: KnowledgebaseUploadResult;
};

const MAX_CONCURRENCY = 3;
const UPLOAD_ACCEPT = ".md,.markdown,.txt,text/plain,text/markdown";
const DOCUMENT_PAGE_SIZE_OPTIONS = [10, 25, 50, 100];
const DEFAULT_DOCUMENT_PAGE_SIZE = 25;

function makeId() {
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function basenameWithoutExtension(name: string) {
  const raw = String(name || "").trim();
  if (!raw) return "";
  const parts = raw.split("/");
  const file = parts[parts.length - 1] || raw;
  return file.replace(/\.[^.]+$/, "");
}

function toDocumentList(payload: any): KnowledgebaseDocument[] {
  if (Array.isArray(payload)) return payload as KnowledgebaseDocument[];
  if (Array.isArray(payload?.documents)) return payload.documents as KnowledgebaseDocument[];
  if (Array.isArray(payload?.items)) return payload.items as KnowledgebaseDocument[];
  if (payload?.document && typeof payload.document === "object") {
    return [payload.document as KnowledgebaseDocument];
  }
  return [];
}

function docCollectionId(document: KnowledgebaseDocument) {
  const explicit = String(document.collection_id || "").trim();
  if (explicit) return explicit;
  const docId = String(document.doc_id || document.id || "").trim();
  const slash = docId.indexOf("/");
  return slash > 0 ? docId.slice(0, slash) : "";
}

function documentIdentity(document: KnowledgebaseDocument) {
  const collection = docCollectionId(document);
  const docId = String(document.doc_id || document.id || "").trim();
  if (!collection || !docId) return "";
  return `${collection}|${docId}`;
}

function formatKbDate(value?: number) {
  const numeric = Number(value || 0);
  if (!numeric) return "n/a";
  return new Date(numeric).toLocaleString();
}

function formatKbBytes(value?: number) {
  const numeric = Number(value || 0);
  if (!numeric) return "n/a";
  if (numeric < 1024) return `${numeric} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = numeric / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(size >= 10 ? 0 : 1)} ${units[unit]}`;
}

function inferDocumentPreviewKind(document?: KnowledgebaseDocument | null) {
  const contentType = String(document?.content_type || "").toLowerCase();
  const title = String(
    document?.title || document?.filename || document?.file_name || ""
  ).toLowerCase();
  if (contentType.includes("markdown") || title.endsWith(".md") || title.endsWith(".markdown")) {
    return "markdown";
  }
  if (contentType.includes("json") || title.endsWith(".json")) {
    return "json";
  }
  return "text";
}

function getDocumentSlug(document?: KnowledgebaseDocument | null) {
  const collection = docCollectionId(document || {});
  const docId = String(document?.doc_id || document?.id || "").trim();
  if (!collection || !docId) return "";
  if (docId.startsWith(`${collection}/`)) return docId.slice(collection.length + 1);
  return docId;
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

export function KnowledgebaseUploadPanel({
  api,
  toast,
  defaultCollectionId,
}: {
  api: (path: string, init?: RequestInit) => Promise<any>;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
  defaultCollectionId?: string;
}) {
  const queryClient = useQueryClient();
  const panelRef = useRef<HTMLDivElement | null>(null);
  const uploadInputRef = useRef<HTMLInputElement | null>(null);
  const folderInputRef = useRef<HTMLInputElement | null>(null);
  const uploadClearTimersRef = useRef<Map<string, number>>(new Map());
  const uploadClearOrderRef = useRef<Map<string, number>>(new Map());
  const uploadClearSequenceRef = useRef(0);
  const [collectionId, setCollectionId] = useState("");
  const [collectionTouched, setCollectionTouched] = useState(false);
  const [panelCollapsed, setPanelCollapsed] = useState(false);
  const [documentSearch, setDocumentSearch] = useState("");
  const [selectedDocumentKey, setSelectedDocumentKey] = useState("");
  const [previewExpanded, setPreviewExpanded] = useState(false);
  const [editMode, setEditMode] = useState(false);
  const [editDraft, setEditDraft] = useState("");
  const [editError, setEditError] = useState("");
  const [documentPage, setDocumentPage] = useState(1);
  const [documentPageSize, setDocumentPageSize] = useState(DEFAULT_DOCUMENT_PAGE_SIZE);
  const [selectedDocumentRef, setSelectedDocumentRef] =
    useState<KnowledgebaseDocumentSelection | null>(null);
  const [selectedDocumentRecords, setSelectedDocumentRecords] = useState<
    KnowledgebaseDocumentSelection[]
  >([]);
  const [deleteConfirm, setDeleteConfirm] = useState<{
    collection: string;
    slug: string;
    title: string;
  } | null>(null);
  const [bulkDeleteConfirm, setBulkDeleteConfirm] = useState<{
    documents: Array<{ collection: string; slug: string; title: string; key: string }>;
  } | null>(null);
  const [rows, setRows] = useState<UploadRow[]>([]);
  const [isUploading, setIsUploading] = useState(false);

  const kbConfigQuery = useQuery({
    queryKey: ["knowledgebase", "config"],
    queryFn: async () => api("/api/knowledgebase/config").catch(() => ({ configured: false })),
    staleTime: 60_000,
  });
  const knowledgebaseAvailable = kbConfigQuery.data?.configured === true;

  const collectionsQuery = useQuery({
    queryKey: ["knowledgebase", "collections"],
    enabled: knowledgebaseAvailable,
    queryFn: async () => api("/api/knowledgebase/collections").catch(() => ({ collections: [] })),
    staleTime: 30_000,
    refetchInterval: 60_000,
  });

  const queryCollectionsRaw = Array.isArray(collectionsQuery.data?.collections)
    ? (collectionsQuery.data.collections as KnowledgebaseCollection[])
    : [];

  useEffect(() => {
    if (collectionTouched) return;
    const candidate = String(defaultCollectionId || "").trim();
    if (!collectionId.trim() && candidate) setCollectionId(candidate);
  }, [collectionId, collectionTouched, defaultCollectionId]);

  useEffect(() => {
    if (collectionTouched) return;
    if (collectionId.trim()) return;
    const first = String(queryCollectionsRaw[0]?.collection_id || "").trim();
    if (first) setCollectionId(first);
  }, [collectionId, collectionTouched, queryCollectionsRaw]);

  useEffect(() => {
    const input = folderInputRef.current;
    if (!input) return;
    input.setAttribute("webkitdirectory", "");
    input.setAttribute("directory", "");
  }, []);

  const currentCollection = collectionId.trim();
  useEffect(() => {
    setDocumentPage(1);
    setSelectedDocumentRecords([]);
    setSelectedDocumentRef(null);
    setSelectedDocumentKey("");
    setPreviewExpanded(true);
    setEditMode(false);
    setEditError("");
  }, [currentCollection]);

  useEffect(() => {
    setDocumentPage(1);
  }, [documentSearch]);

  const currentDocumentOffset = (Math.max(1, documentPage) - 1) * Math.max(1, documentPageSize);
  const documentSearchQuery = documentSearch.trim();
  const documentsQuery = useQuery({
    queryKey: [
      "knowledgebase",
      "documents",
      currentCollection || "__all__",
      documentSearchQuery,
      documentPage,
      documentPageSize,
    ],
    enabled: knowledgebaseAvailable,
    queryFn: async () => {
      const params = new URLSearchParams();
      if (currentCollection) params.set("collection_id", currentCollection);
      params.set("limit", String(Math.max(1, documentPageSize)));
      params.set("offset", String(Math.max(0, currentDocumentOffset)));
      if (documentSearchQuery) params.set("query", documentSearchQuery);
      return api(`/api/knowledgebase/documents?${params.toString()}`).catch(() => ({
        collection_id: currentCollection || null,
        documents: [],
        total: 0,
        offset: currentDocumentOffset,
        limit: documentPageSize,
        has_more: false,
      }));
    },
    staleTime: 15_000,
    refetchInterval: currentCollection ? 30_000 : false,
  });
  const documentPageData = (documentsQuery.data || {}) as KnowledgebaseDocumentPage;
  const documents = useMemo(() => toDocumentList(documentPageData), [documentPageData]);
  const documentTotal = Math.max(0, Number(documentPageData.total ?? documents.length ?? 0));
  const documentLimit = Math.max(
    1,
    Number(documentPageData.limit ?? documentPageSize) || documentPageSize
  );
  const documentOffset = Math.max(
    0,
    Number(documentPageData.offset ?? currentDocumentOffset) || currentDocumentOffset
  );
  const visibleDocumentPageCount = Math.max(
    1,
    Math.ceil(Math.max(documentTotal, documents.length) / Math.max(1, documentPageSize))
  );
  const safeDocumentPage = clampPage(documentPage, visibleDocumentPageCount);
  const documentPageStart = documentOffset;
  const pagedDocuments = documents;
  const documentPageLabel = formatPageWindow(safeDocumentPage, documentLimit, documentTotal);
  useEffect(() => {
    if (documentPage !== safeDocumentPage) setDocumentPage(safeDocumentPage);
  }, [documentPage, safeDocumentPage]);
  useEffect(() => {
    if (selectedDocumentKey) setPreviewExpanded(true);
  }, [selectedDocumentKey]);

  const queryCollections = queryCollectionsRaw;
  const collections = useMemo(
    () =>
      [...queryCollections].sort((a, b) =>
        String(a.collection_id).localeCompare(String(b.collection_id))
      ),
    [queryCollections]
  );
  const collectionOptions = useMemo(() => {
    const byId = new Map<string, KnowledgebaseCollection>();
    const addCollection = (collection: KnowledgebaseCollection | string | undefined) => {
      const collectionId =
        typeof collection === "string"
          ? collection.trim()
          : String(collection?.collection_id || "").trim();
      if (!collectionId || byId.has(collectionId)) return;
      byId.set(
        collectionId,
        typeof collection === "string"
          ? { collection_id: collectionId }
          : { ...collection, collection_id: collectionId }
      );
    };
    addCollection(defaultCollectionId);
    for (const collection of collections) addCollection(collection);
    addCollection(currentCollection);
    return Array.from(byId.values()).sort((a, b) =>
      String(a.collection_id).localeCompare(String(b.collection_id))
    );
  }, [collections, currentCollection, defaultCollectionId]);
  const collectionSelectValue = collectionOptions.some(
    (collection) => String(collection.collection_id || "").trim() === currentCollection
  )
    ? currentCollection
    : "__custom__";
  const selectedDocumentSet = useMemo(
    () => new Set(selectedDocumentRecords.map((record) => record.key)),
    [selectedDocumentRecords]
  );
  const selectedDocuments = selectedDocumentRecords;
  const selectedDocumentCount = selectedDocuments.length;
  const selectedDocument = useMemo(() => selectedDocumentRef || null, [selectedDocumentRef]);
  const selectedDocumentSlug = String(selectedDocument?.slug || "").trim();
  const selectedDocumentCollection = String(selectedDocument?.collection || "").trim();
  const selectedDocumentQuery = useQuery({
    queryKey: ["knowledgebase", "document", selectedDocumentCollection, selectedDocumentSlug],
    enabled: knowledgebaseAvailable && !!selectedDocumentCollection && !!selectedDocumentSlug,
    queryFn: async () =>
      api(
        `/api/knowledgebase/documents/${encodeURIComponent(selectedDocumentCollection)}/${encodeURIComponent(
          selectedDocumentSlug
        )}`
      ),
    staleTime: 30_000,
  });
  const selectedDocumentDetail =
    selectedDocumentQuery.data?.document || selectedDocumentQuery.data || null;
  const selectedPreviewKind = inferDocumentPreviewKind(selectedDocumentDetail || selectedDocument);
  const selectedPreviewTitle = String(
    selectedDocumentDetail?.title ||
      selectedDocumentDetail?.filename ||
      selectedDocumentDetail?.file_name ||
      selectedDocument?.title ||
      selectedDocument?.slug ||
      "Document"
  ).trim();
  const selectedPreviewPath = String(
    selectedDocumentDetail?.path ||
      selectedDocumentDetail?.doc_id ||
      (selectedDocument ? `${selectedDocument.collection}/${selectedDocument.slug}` : "") ||
      ""
  ).trim();
  const selectedPreviewContent = String(
    selectedDocumentDetail?.content || selectedDocumentDetail?.excerpt || ""
  );
  const selectedDocumentError = selectedDocumentQuery.error
    ? selectedDocumentQuery.error instanceof Error
      ? selectedDocumentQuery.error.message
      : String(selectedDocumentQuery.error)
    : "";
  const selectedPreviewUpdatedAt = Number(
    selectedDocumentDetail?.updated_at || selectedDocumentDetail?.created_at || 0
  );
  const selectedPreviewSizeBytes = Number(selectedDocumentDetail?.size_bytes || 0);
  const selectedDocumentCanMutate = Boolean(selectedDocumentCollection && selectedDocumentSlug);
  const selectedDocumentPanel = selectedDocument ? (
    <div className="flex min-h-0 flex-col rounded-xl border border-white/10 bg-black/20 p-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-slate-100">
            {selectedPreviewTitle}
          </div>
          <div className="tcp-subtle mt-1 truncate text-xs">
            {selectedPreviewPath || selectedDocumentSlug || currentCollection}
          </div>
        </div>
        <div className="flex flex-col items-end gap-2">
          <Badge tone="info">{selectedDocumentCollection || currentCollection}</Badge>
          <div className="flex flex-wrap justify-end gap-2">
            <button
              type="button"
              className="tcp-btn h-7 w-7 justify-center px-0"
              title="Refresh preview"
              aria-label="Refresh preview"
              onClick={() => void selectedDocumentQuery.refetch()}
              disabled={!selectedDocumentKey}
            >
              <i data-lucide="refresh-cw"></i>
              <span className="sr-only">Refresh preview</span>
            </button>
            <button
              type="button"
              className="tcp-btn h-7 w-7 justify-center px-0"
              title="Copy document content"
              aria-label="Copy document content"
              onClick={() => void copySelectedDocument()}
              disabled={!selectedPreviewContent.trim()}
            >
              <i data-lucide="copy"></i>
              <span className="sr-only">Copy document content</span>
            </button>
            <button
              type="button"
              className="tcp-btn h-7 w-7 justify-center border-rose-500/30 px-0 text-rose-200 hover:bg-rose-950/20"
              title="Delete document"
              aria-label="Delete document"
              onClick={() =>
                setDeleteConfirm({
                  collection: selectedDocumentCollection,
                  slug: selectedDocumentSlug,
                  title: selectedPreviewTitle,
                })
              }
              disabled={!selectedDocumentCanMutate}
            >
              <i data-lucide="trash-2"></i>
              <span className="sr-only">Delete document</span>
            </button>
          </div>
        </div>
      </div>

      <div className="mt-3 grid grid-cols-3 gap-2 text-xs">
        <div className="rounded-lg border border-white/10 bg-black/10 p-2">
          <div className="tcp-subtle">Type</div>
          <div className="mt-1 font-medium">
            {String(selectedDocumentDetail?.content_type || selectedPreviewKind || "text")}
          </div>
        </div>
        <div className="rounded-lg border border-white/10 bg-black/10 p-2">
          <div className="tcp-subtle">Updated</div>
          <div className="mt-1 font-medium">{formatKbDate(selectedPreviewUpdatedAt)}</div>
        </div>
        <div className="rounded-lg border border-white/10 bg-black/10 p-2">
          <div className="tcp-subtle">Size</div>
          <div className="mt-1 font-medium">{formatKbBytes(selectedPreviewSizeBytes)}</div>
        </div>
      </div>

      <div className="mt-3 rounded-xl border border-white/10 bg-black/10 p-3 text-xs">
        <div className="tcp-subtle uppercase tracking-wide">Excerpt</div>
        <div className="mt-1 whitespace-pre-wrap break-words text-slate-200">
          {String(
            selectedDocumentDetail?.excerpt ||
              selectedPreviewContent.slice(0, 600) ||
              "No excerpt returned."
          )}
        </div>
      </div>

      {selectedDocumentQuery.isLoading && !selectedDocumentDetail ? (
        <div className="mt-3 rounded-xl border border-white/10 bg-black/20 p-3 text-sm tcp-subtle">
          Loading preview...
        </div>
      ) : selectedDocumentError ? (
        <div className="mt-3">
          <EmptyState title="Preview unavailable" text={selectedDocumentError} />
        </div>
      ) : editMode ? (
        <div className="mt-3 flex min-h-0 flex-1 flex-col gap-3 rounded-xl border border-sky-500/30 bg-sky-950/10 p-3">
          <div className="flex items-center justify-between gap-3">
            <div className="tcp-subtle text-xs uppercase tracking-wide">Edit document in place</div>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="tcp-btn h-7 px-2 text-xs"
                onClick={() => {
                  setEditMode(false);
                  setEditError("");
                  setEditDraft(String(selectedPreviewContent || ""));
                }}
              >
                Cancel
              </button>
              <button
                type="button"
                className="tcp-btn h-7 px-2 text-xs"
                onClick={() => void saveSelectedDocument()}
              >
                <i data-lucide="save"></i>
                Save
              </button>
            </div>
          </div>
          {editError ? (
            <div className="rounded-lg border border-rose-500/30 bg-rose-950/20 p-2 text-xs text-rose-200">
              {editError}
            </div>
          ) : null}
          <textarea
            className="tcp-input min-h-[340px] flex-1 resize-none font-mono text-xs leading-6"
            value={editDraft}
            onChange={(event) => setEditDraft(event.target.value)}
            spellCheck={false}
          />
        </div>
      ) : previewExpanded ? (
        <div className="mt-3 flex min-h-0 flex-1 flex-col gap-3 rounded-xl border border-white/10 bg-black/20 p-3">
          <div className="flex items-center justify-between gap-3">
            <div className="tcp-subtle text-xs uppercase tracking-wide">Document content</div>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="tcp-btn h-7 w-7 justify-center px-0"
                title="Edit document"
                aria-label="Edit document"
                onClick={() => {
                  setEditError("");
                  setEditMode(true);
                  setEditDraft(String(selectedPreviewContent || ""));
                }}
                disabled={!selectedPreviewContent.trim()}
              >
                <i data-lucide="square-pen"></i>
                <span className="sr-only">Edit document</span>
              </button>
              <button
                type="button"
                className="tcp-btn h-7 w-7 justify-center px-0"
                title="Collapse preview"
                aria-label="Collapse preview"
                onClick={() => setPreviewExpanded(false)}
              >
                <i data-lucide="chevron-up"></i>
                <span className="sr-only">Collapse preview</span>
              </button>
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-auto rounded-xl border border-white/10 bg-black/20 p-3">
            {selectedPreviewContent ? (
              selectedPreviewKind === "markdown" ? (
                <div
                  className="tcp-markdown tcp-markdown-ai"
                  dangerouslySetInnerHTML={{
                    __html: renderMarkdownSafe(selectedPreviewContent),
                  }}
                />
              ) : (
                <pre className="tcp-code whitespace-pre-wrap break-words">
                  {selectedPreviewContent}
                </pre>
              )
            ) : (
              <EmptyState
                title="No preview content"
                text="This document did not return preview text from the KB service."
              />
            )}
          </div>
        </div>
      ) : (
        <EmptyState
          title="Preview collapsed"
          text="Open the document content when you want to inspect or edit it. The list stays compact until you expand a file."
          action={
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="tcp-btn h-8 w-8 justify-center px-0"
                title="Open content"
                aria-label="Open content"
                onClick={() => setPreviewExpanded(true)}
              >
                <i data-lucide="chevron-down"></i>
                <span className="sr-only">Open content</span>
              </button>
              <button
                type="button"
                className="tcp-btn h-8 w-8 justify-center px-0"
                title="Edit in place"
                aria-label="Edit in place"
                onClick={() => {
                  setPreviewExpanded(true);
                  setEditError("");
                  setEditMode(true);
                  setEditDraft(String(selectedPreviewContent || ""));
                }}
                disabled={!selectedPreviewContent.trim()}
              >
                <i data-lucide="square-pen"></i>
                <span className="sr-only">Edit in place</span>
              </button>
            </div>
          }
        />
      )}
    </div>
  ) : null;

  useEffect(() => {
    if (editMode) return;
    setEditDraft(String(selectedPreviewContent || ""));
  }, [editMode, selectedPreviewContent, selectedDocumentKey]);

  const completedRows = rows.filter((row) => row.status === "done" || row.status === "error");

  useEffect(() => {
    for (const row of rows) {
      if (row.status !== "done" && row.status !== "error") continue;
      if (uploadClearTimersRef.current.has(row.id)) continue;
      const order = uploadClearOrderRef.current.get(row.id) ?? uploadClearSequenceRef.current++ % 6;
      uploadClearOrderRef.current.set(row.id, order);
      const delayMs = 3000 + order * 350;
      const timeoutId = window.setTimeout(() => {
        setRows((prev) => prev.filter((entry) => entry.id !== row.id));
        uploadClearTimersRef.current.delete(row.id);
        uploadClearOrderRef.current.delete(row.id);
      }, delayMs);
      uploadClearTimersRef.current.set(row.id, timeoutId);
    }

    const activeRowIds = new Set(rows.map((row) => row.id));
    for (const [rowId, timeoutId] of uploadClearTimersRef.current.entries()) {
      if (activeRowIds.has(rowId)) continue;
      window.clearTimeout(timeoutId);
      uploadClearTimersRef.current.delete(rowId);
      uploadClearOrderRef.current.delete(rowId);
    }
  }, [rows]);

  useEffect(() => {
    return () => {
      for (const timeoutId of uploadClearTimersRef.current.values()) {
        window.clearTimeout(timeoutId);
      }
      uploadClearTimersRef.current.clear();
      uploadClearOrderRef.current.clear();
    };
  }, []);

  useEffect(() => {
    if (panelRef.current) renderIcons(panelRef.current);
  }, [
    collections.length,
    collectionOptions.length,
    rows.length,
    documentPage,
    documentPageSize,
    documentTotal,
    documentOffset,
    documentLimit,
    currentCollection,
    isUploading,
    knowledgebaseAvailable,
    documents.length,
    pagedDocuments.length,
    selectedDocumentKey,
    selectedDocumentRef,
    selectedDocumentCount,
    documentSearch,
    selectedDocumentQuery.data,
    selectedDocumentQuery.isFetching,
    previewExpanded,
    editMode,
    panelCollapsed,
  ]);

  const clearFinishedUploads = () => {
    for (const row of rows) {
      if (row.status !== "done" && row.status !== "error") continue;
      const timeoutId = uploadClearTimersRef.current.get(row.id);
      if (typeof timeoutId === "number") window.clearTimeout(timeoutId);
      uploadClearTimersRef.current.delete(row.id);
      uploadClearOrderRef.current.delete(row.id);
    }
    setRows((prev) => prev.filter((row) => row.status === "queued" || row.status === "uploading"));
  };

  const uploadOne = useMutation({
    mutationFn: ({ file, targetCollection }: { file: File; targetCollection: string }) =>
      new Promise<any>((resolve, reject) => {
        const id = makeId();
        setRows((prev) => [
          ...prev,
          {
            id,
            name: file.name,
            progress: 0,
            status: "queued",
            error: "",
          },
        ]);

        const xhr = new XMLHttpRequest();
        xhr.open("POST", "/api/knowledgebase/documents");
        xhr.withCredentials = true;
        xhr.responseType = "json";

        const form = new FormData();
        form.set("collection_id", targetCollection);
        form.set("title", basenameWithoutExtension(file.name));
        form.set("file", file, file.name);

        xhr.upload.onprogress = (event) => {
          if (!event.lengthComputable) return;
          const pct = (event.loaded / event.total) * 100;
          setRows((prev) =>
            prev.map((row) =>
              row.id === id ? { ...row, status: "uploading", progress: pct } : row
            )
          );
        };

        xhr.onerror = () => {
          const message = "Network error";
          setRows((prev) =>
            prev.map((row) =>
              row.id === id
                ? { ...row, status: "error", error: message, progress: Math.max(row.progress, 4) }
                : row
            )
          );
          reject(new Error(`KB upload failed: ${file.name}`));
        };

        xhr.onload = () => {
          const payload = xhr.response || {};
          if (xhr.status < 200 || xhr.status >= 300 || payload?.ok === false) {
            const message = String(payload?.error || `Upload failed (${xhr.status})`);
            setRows((prev) =>
              prev.map((row) =>
                row.id === id
                  ? { ...row, status: "error", error: message, progress: Math.max(row.progress, 4) }
                  : row
              )
            );
            reject(new Error(message));
            return;
          }

          const document = (
            payload?.document && typeof payload.document === "object"
              ? payload.document
              : payload?.doc || payload?.item || {}
          ) as Record<string, any>;
          const nextDocId = String(
            document?.doc_id || document?.id || payload?.doc_id || ""
          ).trim();
          const nextCollectionId = String(document?.collection_id || targetCollection).trim();
          const nextTitle = String(
            document?.title ||
              document?.filename ||
              document?.file_name ||
              basenameWithoutExtension(file.name) ||
              file.name
          ).trim();

          setRows((prev) =>
            prev.map((row) =>
              row.id === id
                ? {
                    ...row,
                    status: "done",
                    progress: 100,
                    error: "",
                    result: {
                      docId: nextDocId,
                      collectionId: nextCollectionId,
                      title: nextTitle,
                    },
                  }
                : row
            )
          );
          resolve(payload);
        };

        setRows((prev) =>
          prev.map((row) => (row.id === id ? { ...row, status: "uploading" } : row))
        );
        xhr.send(form);
      }),
  });

  const uploadFiles = async (fileList: FileList | null) => {
    const files = [...(fileList || [])];
    const targetCollection = collectionId.trim();
    if (!knowledgebaseAvailable || !files.length) return;
    if (!targetCollection) {
      toast("warn", "Choose a knowledgebase collection first.");
      return;
    }

    setIsUploading(true);
    let okCount = 0;
    let failCount = 0;
    let cursor = 0;

    try {
      const workers = Array.from({ length: Math.min(MAX_CONCURRENCY, files.length) }, async () => {
        while (true) {
          const index = cursor;
          cursor += 1;
          if (index >= files.length) return;
          const file = files[index];
          try {
            await uploadOne.mutateAsync({ file, targetCollection });
            okCount += 1;
          } catch {
            failCount += 1;
          }
        }
      });
      await Promise.all(workers);
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "collections"] });
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "documents"] });
      if (okCount && failCount) {
        toast("warn", `Uploaded ${okCount} file(s) to ${targetCollection}; ${failCount} failed.`);
      } else if (okCount) {
        toast("ok", `Uploaded ${okCount} file(s) to ${targetCollection}.`);
      } else if (failCount) {
        toast("err", `All ${failCount} KB upload(s) failed.`);
      }
    } finally {
      setIsUploading(false);
    }
  };

  const reindex = async () => {
    try {
      await api("/api/knowledgebase/reindex", { method: "POST" });
      toast("ok", "Knowledgebase reindex requested.");
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  const toDocumentSelection = (
    document: KnowledgebaseDocument
  ): KnowledgebaseDocumentSelection | null => {
    const collection = String(docCollectionId(document) || currentCollection).trim();
    const slug = getDocumentSlug(document);
    const key = documentIdentity(document);
    if (!collection || !slug || !key) return null;
    return {
      collection,
      slug,
      title: String(
        document.title || document.filename || document.file_name || document.doc_id || slug
      ).trim(),
      key,
    };
  };

  const toggleDocumentSelection = (document: KnowledgebaseDocument) => {
    const selection = toDocumentSelection(document);
    if (!selection) return;
    setSelectedDocumentRecords((current) =>
      current.some((entry) => entry.key === selection.key)
        ? current.filter((entry) => entry.key !== selection.key)
        : [...current, selection]
    );
  };

  const selectVisibleDocuments = () => {
    const selections = documents
      .map((document) => toDocumentSelection(document))
      .filter((value): value is KnowledgebaseDocumentSelection => !!value);
    setSelectedDocumentRecords((current) => {
      const map = new Map(current.map((entry) => [entry.key, entry]));
      for (const selection of selections) {
        map.set(selection.key, selection);
      }
      return [...map.values()];
    });
  };

  const clearSelectedDocuments = () => {
    setSelectedDocumentRecords([]);
  };

  const copySelectedDocument = async () => {
    const text = String(selectedPreviewContent || "").trim();
    if (!text) {
      toast("warn", "No document content is available to copy.");
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      toast("ok", "Document content copied to clipboard.");
    } catch {
      toast("warn", "Could not copy the document content.");
    }
  };

  const saveSelectedDocument = async () => {
    const collection = String(selectedDocumentCollection || "").trim();
    const slug = String(selectedDocumentSlug || "").trim();
    if (!collection || !slug) {
      toast("warn", "Pick a document before saving edits.");
      return;
    }
    setEditError("");
    try {
      const form = new FormData();
      form.set("title", String(selectedPreviewTitle || "").trim() || slug);
      form.set("content", editDraft);
      const response = await fetch(
        `/api/knowledgebase/documents/${encodeURIComponent(collection)}/${encodeURIComponent(slug)}`,
        {
          method: "PUT",
          credentials: "include",
          body: form,
        }
      );
      const text = await response.text();
      let payload: any = {};
      if (text) {
        try {
          payload = JSON.parse(text);
        } catch {
          payload = {};
        }
      }
      if (!response.ok || payload?.ok === false) {
        const message = String(payload?.error || text || `Update failed (${response.status})`);
        throw new Error(message);
      }
      setEditMode(false);
      setPreviewExpanded(true);
      toast("ok", "Document updated.");
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "documents"] });
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "document"] });
      await documentsQuery.refetch();
      await selectedDocumentQuery.refetch();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setEditError(message);
      toast("err", message);
    }
  };

  const openBulkDeleteDialog = () => {
    if (!selectedDocuments.length) {
      toast("warn", "Select one or more documents first.");
      return;
    }
    setBulkDeleteConfirm({
      documents: selectedDocuments.map(({ collection, slug, title, key }) => ({
        collection,
        slug,
        title,
        key,
      })),
    });
  };

  const confirmDeleteSelectedDocument = async () => {
    const collection = String(deleteConfirm?.collection || "").trim();
    const slug = String(deleteConfirm?.slug || "").trim();
    if (!collection || !slug) {
      toast("warn", "Pick a document before deleting it.");
      return;
    }
    try {
      const response = await fetch(
        `/api/knowledgebase/documents/${encodeURIComponent(collection)}/${encodeURIComponent(slug)}`,
        {
          method: "DELETE",
          credentials: "include",
        }
      );
      const text = await response.text();
      let payload: any = {};
      if (text) {
        try {
          payload = JSON.parse(text);
        } catch {
          payload = {};
        }
      }
      if (!response.ok || payload?.ok === false) {
        const message = String(payload?.error || text || `Delete failed (${response.status})`);
        throw new Error(message);
      }
      const deletedKey = selectedDocumentRef?.key || "";
      setSelectedDocumentKey("");
      setSelectedDocumentRef(null);
      setSelectedDocumentRecords((current) => current.filter((entry) => entry.key !== deletedKey));
      setPreviewExpanded(false);
      setEditMode(false);
      setEditDraft("");
      setEditError("");
      setDeleteConfirm(null);
      toast("ok", "Document deleted.");
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "collections"] });
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "documents"] });
      await documentsQuery.refetch();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast("err", message);
    }
  };

  const confirmDeleteSelectedDocuments = async () => {
    const targets = bulkDeleteConfirm?.documents || [];
    if (!targets.length) {
      toast("warn", "Select one or more documents first.");
      return;
    }

    const remainingSelections = new Map(selectedDocumentRecords.map((entry) => [entry.key, entry]));
    let okCount = 0;
    let failCount = 0;

    try {
      for (const target of targets) {
        try {
          const response = await fetch(
            `/api/knowledgebase/documents/${encodeURIComponent(target.collection)}/${encodeURIComponent(target.slug)}`,
            {
              method: "DELETE",
              credentials: "include",
            }
          );
          const text = await response.text();
          let payload: any = {};
          if (text) {
            try {
              payload = JSON.parse(text);
            } catch {
              payload = {};
            }
          }
          if (!response.ok || payload?.ok === false) {
            const message = String(payload?.error || text || `Delete failed (${response.status})`);
            throw new Error(message);
          }
          okCount += 1;
          remainingSelections.delete(target.key);
        } catch {
          failCount += 1;
        }
      }

      setSelectedDocumentRecords([...remainingSelections.values()]);
      if (selectedDocumentRef?.key && !remainingSelections.has(selectedDocumentRef.key)) {
        setSelectedDocumentKey("");
        setSelectedDocumentRef(null);
        setPreviewExpanded(false);
        setEditMode(false);
        setEditDraft("");
        setEditError("");
      }
      setBulkDeleteConfirm(null);
      if (okCount && failCount) {
        toast("warn", `Deleted ${okCount} document(s); ${failCount} failed.`);
      } else if (okCount) {
        toast("ok", `Deleted ${okCount} document(s).`);
      } else {
        toast("err", `All ${failCount} selected document deletions failed.`);
      }
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "collections"] });
      await queryClient.invalidateQueries({ queryKey: ["knowledgebase", "documents"] });
      await documentsQuery.refetch();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast("err", message);
    }
  };

  if (!knowledgebaseAvailable) return null;

  return (
    <PanelCard
      className="overflow-hidden"
      title="Knowledgebase"
      subtitle="Provisioned-server docs live in KB collections searched through MCP, not in the file buckets below."
      actions={
        <Toolbar className="justify-start">
          <button
            type="button"
            className="tcp-btn"
            onClick={() => uploadInputRef.current?.click()}
            disabled={isUploading}
          >
            <i data-lucide="upload"></i>
            Upload docs
          </button>
          <button
            type="button"
            className="tcp-btn"
            onClick={() => folderInputRef.current?.click()}
            disabled={isUploading}
          >
            <i data-lucide="folder-open"></i>
            Upload folder
          </button>
          <button type="button" className="tcp-btn" onClick={() => void collectionsQuery.refetch()}>
            <i data-lucide="refresh-cw"></i>
            Refresh
          </button>
          <button type="button" className="tcp-btn" onClick={reindex}>
            <i data-lucide="sparkles"></i>
            Reindex
          </button>
          <button
            type="button"
            className="tcp-btn"
            onClick={() => setPanelCollapsed((current) => !current)}
          >
            <i data-lucide={panelCollapsed ? "chevron-down" : "chevron-up"}></i>
            {panelCollapsed ? "Expand" : "Collapse"}
          </button>
        </Toolbar>
      }
    >
      <div ref={panelRef} className={panelCollapsed ? "hidden" : "grid gap-4 p-4"}>
        <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
          <div className="grid gap-2">
            <label
              className="tcp-subtle text-xs uppercase tracking-wide"
              htmlFor="kb-collection-select"
            >
              Collection
            </label>
            <div className="grid gap-2 md:grid-cols-[minmax(180px,0.7fr)_minmax(220px,1fr)]">
              <select
                id="kb-collection-select"
                className="tcp-input"
                value={collectionSelectValue}
                onChange={(event) => {
                  const next = event.target.value;
                  setCollectionTouched(true);
                  if (next === "__custom__") {
                    setCollectionId("");
                    return;
                  }
                  setCollectionId(next);
                }}
              >
                {collectionOptions.length ? (
                  collectionOptions.map((collection) => {
                    const name = String(collection.collection_id || "").trim();
                    return (
                      <option key={name} value={name}>
                        {name}
                        {typeof collection.document_count === "number"
                          ? ` (${collection.document_count})`
                          : ""}
                      </option>
                    );
                  })
                ) : (
                  <option value="">No collections yet</option>
                )}
                <option value="__custom__">New collection...</option>
              </select>
              <input
                id="kb-collection-id"
                className="tcp-input"
                value={collectionId}
                onChange={(event) => {
                  setCollectionTouched(true);
                  setCollectionId(event.target.value);
                }}
                placeholder="new-customer-collection"
                spellCheck={false}
              />
            </div>
            <div className="tcp-subtle text-xs">
              Pick an existing KB MCP collection, or type a new collection ID before uploading.
            </div>
          </div>
          <div className="flex items-end gap-2">
            <input
              ref={uploadInputRef}
              type="file"
              multiple
              accept={UPLOAD_ACCEPT}
              className="hidden"
              onChange={(event) => {
                void uploadFiles(event.target.files);
                event.currentTarget.value = "";
              }}
            />
            <input
              ref={folderInputRef}
              type="file"
              multiple
              accept={UPLOAD_ACCEPT}
              className="hidden"
              onChange={(event) => {
                void uploadFiles(event.target.files);
                event.currentTarget.value = "";
              }}
            />
            <button
              type="button"
              className="tcp-btn h-10"
              onClick={() => uploadInputRef.current?.click()}
              disabled={!currentCollection || isUploading}
            >
              <i data-lucide="files"></i>
              Select files
            </button>
            <button
              type="button"
              className="tcp-btn h-10"
              onClick={() => folderInputRef.current?.click()}
              disabled={!currentCollection || isUploading}
              title="Select a local folder and upload all matching docs inside it"
            >
              <i data-lucide="folder-open"></i>
              Select folder
            </button>
          </div>
        </div>

        <div className="flex flex-wrap gap-2">
          {collections.length ? (
            collections.slice(0, 8).map((collection) => {
              const name = String(collection.collection_id || "").trim();
              const active = name === currentCollection;
              return (
                <button
                  key={name}
                  type="button"
                  className={`tcp-btn h-7 px-2 text-xs ${active ? "border-sky-500/40 bg-sky-950/20" : ""}`.trim()}
                  onClick={() => {
                    setCollectionTouched(true);
                    setCollectionId(name);
                  }}
                >
                  {name}
                  {typeof collection.document_count === "number" ? (
                    <span className="ml-2 text-[10px] text-slate-400">
                      {collection.document_count}
                    </span>
                  ) : null}
                </button>
              );
            })
          ) : (
            <span className="tcp-subtle text-sm">
              No collections reported yet. Uploads can still succeed before the KB service lists the
              collection.
            </span>
          )}
        </div>

        <div className="rounded-xl border border-white/10 bg-black/20 p-3 text-sm">
          <div className="font-medium text-slate-100">How this works</div>
          <div className="tcp-subtle mt-1">
            The KB MCP reads from collections in the hosted knowledgebase service. It does not
            browse a raw server folder directly, so uploads here will not appear in the managed
            `Files` buckets below unless you upload them there separately.
          </div>
        </div>

        {rows.length ? (
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-3">
              <div className="tcp-subtle text-xs uppercase tracking-wide">Upload history</div>
              {completedRows.length ? (
                <button
                  type="button"
                  className="tcp-btn h-7 px-2 text-xs"
                  onClick={clearFinishedUploads}
                >
                  Clear finished
                </button>
              ) : null}
            </div>
            <div className="grid gap-2">
              {rows.map((row) => (
                <div
                  key={row.id}
                  className="rounded-xl border border-white/10 bg-black/20 p-3 text-xs"
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <div className="truncate font-medium">{row.name}</div>
                      <div className="tcp-subtle mt-1 truncate">
                        {row.result?.collectionId || currentCollection || "collection"} •{" "}
                        {row.status}
                      </div>
                      {row.result?.docId ? (
                        <div className="tcp-subtle mt-1 truncate text-[11px]">
                          {row.result.docId}
                        </div>
                      ) : null}
                    </div>
                    <Badge
                      tone={row.status === "done" ? "ok" : row.status === "error" ? "err" : "info"}
                    >
                      {Math.round(row.progress)}%
                    </Badge>
                  </div>
                  <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-white/10">
                    <div
                      className={`h-full rounded-full ${
                        row.status === "error"
                          ? "bg-rose-400"
                          : row.status === "done"
                            ? "bg-emerald-400"
                            : "bg-sky-400"
                      }`}
                      style={{ width: `${Math.max(4, Math.min(100, row.progress || 0))}%` }}
                    ></div>
                  </div>
                  {row.error ? <div className="mt-2 text-rose-200">{row.error}</div> : null}
                </div>
              ))}
            </div>
          </div>
        ) : null}

        <div className="grid gap-2">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <div className="tcp-subtle text-xs uppercase tracking-wide">Collection documents</div>
              <div className="tcp-subtle mt-1 text-xs">
                Showing what the selected KB collection currently contains.
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Badge tone="ghost">{documentTotal}</Badge>
              <Badge tone={selectedDocumentCount ? "info" : "ghost"}>
                {selectedDocumentCount} selected
              </Badge>
              <button
                type="button"
                className="tcp-btn h-8 px-3 text-xs"
                onClick={() => void documentsQuery.refetch()}
                disabled={!currentCollection}
              >
                <i data-lucide="refresh-cw"></i>
                Refresh docs
              </button>
            </div>
          </div>

          {!currentCollection ? (
            <div className="rounded-xl border border-white/10 bg-black/20 p-3 text-sm tcp-subtle">
              Pick or type a collection ID to inspect its documents.
            </div>
          ) : documentsQuery.isFetching && !documents.length ? (
            <div className="rounded-xl border border-white/10 bg-black/20 p-3 text-sm tcp-subtle">
              Loading documents from{" "}
              <span className="font-medium text-slate-200">{currentCollection}</span>.
            </div>
          ) : documents.length ? (
            <div className="grid gap-4">
              <div className="grid min-h-0 gap-3">
                <div className="grid gap-2 rounded-xl border border-white/10 bg-black/20 p-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <input
                      className="tcp-input h-9 min-w-[220px] flex-1"
                      value={documentSearch}
                      onChange={(event) => setDocumentSearch(event.target.value)}
                      placeholder="Filter documents"
                      spellCheck={false}
                    />
                    <Badge tone="ghost">{documentPageLabel}</Badge>
                  </div>
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex flex-wrap items-center gap-2">
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        onClick={selectVisibleDocuments}
                        disabled={!documents.length}
                      >
                        Select all
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        onClick={clearSelectedDocuments}
                        disabled={!selectedDocumentCount}
                      >
                        Clear selection
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs border-rose-500/30 text-rose-100 hover:bg-rose-950/20 disabled:opacity-50"
                        onClick={openBulkDeleteDialog}
                        disabled={!selectedDocumentCount}
                      >
                        <i data-lucide="trash-2"></i>
                        Delete selected
                      </button>
                    </div>
                    <div className="flex flex-wrap items-center gap-2">
                      <label className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-slate-500">
                        <span>Per page</span>
                        <select
                          className="tcp-select h-8 min-w-[5.5rem] px-3 text-center text-sm font-semibold tabular-nums text-slate-100 [text-align-last:center]"
                          value={documentPageSize}
                          onChange={(event) => {
                            setDocumentPageSize(
                              Number(event.target.value) || DEFAULT_DOCUMENT_PAGE_SIZE
                            );
                            setDocumentPage(1);
                          }}
                        >
                          {DOCUMENT_PAGE_SIZE_OPTIONS.map((value) => (
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
                          setDocumentPage((page) => clampPage(page - 1, visibleDocumentPageCount))
                        }
                        disabled={safeDocumentPage <= 1}
                      >
                        Prev
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        onClick={() =>
                          setDocumentPage((page) => clampPage(page + 1, visibleDocumentPageCount))
                        }
                        disabled={safeDocumentPage >= visibleDocumentPageCount}
                      >
                        Next
                      </button>
                    </div>
                  </div>
                </div>

                {pagedDocuments.length ? (
                  <div className="grid gap-2">
                    {pagedDocuments.map((document, index) => {
                      const docId = String(document.doc_id || document.id || "").trim();
                      const title = String(
                        document.title ||
                          document.filename ||
                          document.file_name ||
                          docId ||
                          `Document ${documentPageStart + index + 1}`
                      ).trim();
                      const collection = String(document.collection_id || currentCollection).trim();
                      const updatedAt = Number(document.updated_at || document.created_at || 0);
                      const identity = documentIdentity(document);
                      const selectable = !!identity;
                      const checked = selectable && selectedDocumentSet.has(identity);
                      const active = identity === selectedDocumentKey;
                      return (
                        <motion.div key={docId || `${collection}-${title}-${index}`} layout>
                          <div className="flex items-start gap-3">
                            <label
                              className={`mt-3 flex h-6 w-6 items-center justify-center rounded border ${
                                selectable
                                  ? "border-white/15 bg-black/20 text-slate-200 cursor-pointer hover:border-sky-500/40"
                                  : "border-white/10 bg-black/10 text-slate-500"
                              }`}
                            >
                              <input
                                type="checkbox"
                                className="h-4 w-4 accent-sky-400"
                                checked={checked}
                                disabled={!selectable}
                                onChange={() => toggleDocumentSelection(document)}
                                aria-label={`Select ${title}`}
                              />
                            </label>

                            <div className="min-w-0 flex-1">
                              <button
                                type="button"
                                className={`w-full rounded-xl border p-3 text-left text-sm transition ${
                                  active
                                    ? "border-sky-500/50 bg-sky-950/20"
                                    : checked
                                      ? "border-emerald-500/40 bg-emerald-950/20 hover:border-emerald-400/50 hover:bg-emerald-950/30"
                                      : "border-white/10 bg-black/20 hover:border-white/20 hover:bg-black/30"
                                }`.trim()}
                                onClick={() => {
                                  const selection = toDocumentSelection(document);
                                  if (active) {
                                    setSelectedDocumentKey("");
                                    setSelectedDocumentRef(null);
                                    setPreviewExpanded(false);
                                    setEditMode(false);
                                    setEditError("");
                                    return;
                                  }
                                  setSelectedDocumentKey(identity);
                                  if (selection) setSelectedDocumentRef(selection);
                                  setPreviewExpanded(true);
                                  setEditMode(false);
                                  setEditError("");
                                }}
                                aria-expanded={active}
                              >
                                <div className="flex items-start justify-between gap-3">
                                  <div className="min-w-0">
                                    <div className="truncate font-medium text-slate-100">
                                      {title}
                                    </div>
                                    <div className="tcp-subtle mt-1 truncate text-xs">
                                      {docId || `${collection}/${title}`}
                                    </div>
                                  </div>
                                  <Badge tone={active ? "ok" : checked ? "info" : "ghost"}>
                                    {collection}
                                  </Badge>
                                </div>
                                <div className="mt-2 flex flex-wrap gap-2 text-xs tcp-subtle">
                                  <span>Updated: {formatKbDate(updatedAt)}</span>
                                  {document.content_type ? (
                                    <span>{document.content_type}</span>
                                  ) : null}
                                </div>
                              </button>

                              <AnimatePresence initial={false}>
                                {active ? (
                                  <motion.div
                                    key={`${selectedDocumentKey}-expanded`}
                                    initial={{ opacity: 0, height: 0, y: -8 }}
                                    animate={{ opacity: 1, height: "auto", y: 0 }}
                                    exit={{ opacity: 0, height: 0, y: -8 }}
                                    transition={{ duration: 0.22, ease: "easeOut" }}
                                    className="overflow-hidden"
                                  >
                                    <div className="mt-2">{selectedDocumentPanel}</div>
                                  </motion.div>
                                ) : null}
                              </AnimatePresence>
                            </div>
                          </div>
                        </motion.div>
                      );
                    })}
                  </div>
                ) : (
                  <EmptyState
                    title={
                      documentSearchQuery ? "No documents match your filter" : "No documents found"
                    }
                    text={
                      documentSearchQuery
                        ? "Try another search term or clear the filter to show the full collection."
                        : "This collection does not contain any documents yet."
                    }
                    action={
                      documentSearchQuery ? (
                        <button
                          type="button"
                          className="tcp-btn h-8 px-3 text-xs"
                          onClick={() => setDocumentSearch("")}
                        >
                          Clear filter
                        </button>
                      ) : null
                    }
                  />
                )}
              </div>
            </div>
          ) : (
            <div className="rounded-xl border border-white/10 bg-black/20 p-3 text-sm tcp-subtle">
              No visible documents returned for{" "}
              <span className="font-medium text-slate-200">{currentCollection}</span>. If you just
              uploaded a file, refresh docs or reindex and confirm the KB admin service exposes
              document listing for that collection.
            </div>
          )}
        </div>

        <ConfirmDialog
          open={!!deleteConfirm}
          title="Delete document"
          message={
            <span>
              This will permanently remove{" "}
              <strong>{deleteConfirm?.title || "this document"}</strong> from{" "}
              <strong>
                {deleteConfirm?.collection || currentCollection || "the selected collection"}
              </strong>
              .
            </span>
          }
          confirmLabel="Delete document"
          confirmIcon="trash-2"
          confirmTone="danger"
          onCancel={() => setDeleteConfirm(null)}
          onConfirm={() => void confirmDeleteSelectedDocument()}
        />

        <ConfirmDialog
          open={!!bulkDeleteConfirm}
          title="Delete selected documents"
          message={
            <span>
              This will permanently remove{" "}
              <strong>{bulkDeleteConfirm?.documents.length || 0} selected document(s)</strong> from{" "}
              <strong>{currentCollection || "the selected collection"}</strong>.
            </span>
          }
          confirmLabel="Delete selected"
          confirmIcon="trash-2"
          confirmTone="danger"
          widthClassName="w-[min(42rem,96vw)]"
          onCancel={() => setBulkDeleteConfirm(null)}
          onConfirm={() => void confirmDeleteSelectedDocuments()}
        >
          {bulkDeleteConfirm?.documents.length ? (
            <div className="mt-3 grid gap-2 rounded-xl border border-white/10 bg-black/20 p-3 text-left text-xs">
              <div className="tcp-subtle uppercase tracking-wide">Selected documents</div>
              <div className="max-h-40 overflow-auto">
                {bulkDeleteConfirm.documents.slice(0, 12).map((document) => (
                  <div key={`${document.collection}|${document.slug}`} className="truncate py-1">
                    <span className="font-medium text-slate-100">
                      {document.title || document.slug}
                    </span>
                    <span className="tcp-subtle">
                      {" "}
                      · {document.collection}/{document.slug}
                    </span>
                  </div>
                ))}
                {bulkDeleteConfirm.documents.length > 12 ? (
                  <div className="pt-1 text-slate-400">
                    +{bulkDeleteConfirm.documents.length - 12} more
                  </div>
                ) : null}
              </div>
            </div>
          ) : null}
        </ConfirmDialog>
      </div>
    </PanelCard>
  );
}
