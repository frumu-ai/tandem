import { useMutation } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import type {
  MemoryImportFormat,
  MemoryImportResponse,
  MemoryImportTier,
  TandemClient,
} from "@frumu/tandem-client";
import { renderIcons } from "../app/icons.js";
import type { ToastKind } from "../pages/pageTypes";

type ImportForm = {
  path: string;
  format: MemoryImportFormat;
  tier: MemoryImportTier;
  projectId: string;
  sessionId: string;
  syncDeletes: boolean;
};

type MemoryImportDialogProps = {
  open: boolean;
  client: TandemClient;
  initialPath?: string;
  initialTier?: MemoryImportTier;
  initialProjectId?: string;
  note?: string;
  toast: (kind: ToastKind, text: string) => void;
  onCancel: () => void;
  onSuccess?: (result: MemoryImportResponse) => void | Promise<void>;
};

const STATS: Array<[keyof MemoryImportResponse, string]> = [
  ["discovered_files", "Discovered files"],
  ["indexed_files", "Indexed files"],
  ["skipped_files", "Skipped files"],
  ["deleted_files", "Deleted files"],
  ["chunks_created", "Chunks created"],
  ["errors", "Errors"],
];

export function MemoryImportDialog({
  open,
  client,
  initialPath = "",
  initialTier = "global",
  initialProjectId = "",
  note = "",
  toast,
  onCancel,
  onSuccess,
}: MemoryImportDialogProps) {
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const [form, setForm] = useState<ImportForm>({
    path: initialPath,
    format: "directory",
    tier: initialTier,
    projectId: initialProjectId,
    sessionId: "",
    syncDeletes: false,
  });
  const [result, setResult] = useState<MemoryImportResponse | null>(null);

  useEffect(() => {
    if (!open) return;
    setForm({
      path: initialPath,
      format: "directory",
      tier: initialTier,
      projectId: initialProjectId,
      sessionId: "",
      syncDeletes: false,
    });
    setResult(null);
  }, [initialPath, initialProjectId, initialTier, open]);

  useEffect(() => {
    if (!open || !dialogRef.current) return;
    renderIcons(dialogRef.current);
  }, [form.format, form.syncDeletes, form.tier, open, result]);

  useEffect(() => {
    if (!open || typeof window === "undefined") return undefined;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel, open]);

  const importMutation = useMutation({
    mutationFn: () =>
      client.memory.importPath({
        path: form.path.trim(),
        format: form.format,
        tier: form.tier,
        projectId: form.projectId.trim() || undefined,
        sessionId: form.sessionId.trim() || undefined,
        syncDeletes: form.syncDeletes,
      }),
    onSuccess: async (payload) => {
      setResult(payload);
      toast(
        "ok",
        `Indexed ${payload.indexed_files} file(s) and created ${payload.chunks_created} chunk(s).`
      );
      await onSuccess?.(payload);
    },
    onError: (error) => {
      toast("err", error instanceof Error ? error.message : String(error));
    },
  });

  const projectRequired = form.tier === "project";
  const sessionRequired = form.tier === "session";
  const pathMissing = !form.path.trim();
  const scopeMissing =
    (projectRequired && !form.projectId.trim()) || (sessionRequired && !form.sessionId.trim());

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="tcp-confirm-overlay"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={onCancel}
        >
          <motion.div
            ref={dialogRef}
            className="tcp-confirm-dialog w-[min(42rem,96vw)]"
            initial={{ opacity: 0, y: 8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6, scale: 0.98 }}
            onClick={(event) => event.stopPropagation()}
          >
            <h3 className="tcp-confirm-title">Import Knowledge</h3>
            <div className="tcp-confirm-message">
              Index this file or folder so Tandem agents can retrieve it as governed memory.
            </div>
            {note ? (
              <div className="mb-3 rounded-lg border border-amber-500/20 bg-amber-950/20 p-2 text-xs text-amber-100">
                {note}
              </div>
            ) : null}

            <div className="grid gap-3 text-left">
              <label className="grid gap-1">
                <span className="text-xs uppercase tracking-wide text-slate-500">Path</span>
                <input
                  className="tcp-input"
                  value={form.path}
                  placeholder="/srv/tandem/imports/company-docs"
                  onChange={(event) => setForm({ ...form, path: event.target.value })}
                />
              </label>

              <div className="grid gap-3 sm:grid-cols-3">
                <label className="grid gap-1">
                  <span className="text-xs uppercase tracking-wide text-slate-500">Format</span>
                  <select
                    className="tcp-select"
                    value={form.format}
                    onChange={(event) =>
                      setForm({ ...form, format: event.target.value as MemoryImportFormat })
                    }
                  >
                    <option value="directory">directory</option>
                    <option value="openclaw">openclaw</option>
                  </select>
                </label>
                <label className="grid gap-1">
                  <span className="text-xs uppercase tracking-wide text-slate-500">Tier</span>
                  <select
                    className="tcp-select"
                    value={form.tier}
                    onChange={(event) =>
                      setForm({ ...form, tier: event.target.value as MemoryImportTier })
                    }
                  >
                    <option value="global">global</option>
                    <option value="project">project</option>
                    <option value="session">session</option>
                  </select>
                </label>
                <label className="flex items-center gap-2 rounded-lg border border-white/10 bg-black/20 px-3 py-2 text-sm">
                  <input
                    type="checkbox"
                    className="h-4 w-4 accent-sky-400"
                    checked={form.syncDeletes}
                    onChange={(event) => setForm({ ...form, syncDeletes: event.target.checked })}
                  />
                  Sync deletes
                </label>
              </div>

              <div className="grid gap-3 sm:grid-cols-2">
                <label className="grid gap-1">
                  <span className="text-xs uppercase tracking-wide text-slate-500">project_id</span>
                  <input
                    className="tcp-input"
                    value={form.projectId}
                    placeholder={projectRequired ? "company-brain-demo" : "optional"}
                    onChange={(event) => setForm({ ...form, projectId: event.target.value })}
                  />
                </label>
                <label className="grid gap-1">
                  <span className="text-xs uppercase tracking-wide text-slate-500">session_id</span>
                  <input
                    className="tcp-input"
                    value={form.sessionId}
                    placeholder={sessionRequired ? "session id" : "optional"}
                    onChange={(event) => setForm({ ...form, sessionId: event.target.value })}
                  />
                </label>
              </div>
            </div>

            {result ? (
              <div className="mt-3 grid gap-2 rounded-xl border border-white/10 bg-black/20 p-3 text-left">
                <div className="text-xs uppercase tracking-wide text-slate-500">Import stats</div>
                <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
                  {STATS.map(([key, label]) => (
                    <div key={key} className="rounded-lg border border-white/10 bg-black/10 p-2">
                      <div className="tcp-subtle text-[11px]">{label}</div>
                      <div className="mt-1 font-semibold tabular-nums">
                        {Number(result[key] || 0)}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ) : null}

            <div className="tcp-confirm-actions mt-3">
              <button type="button" className="tcp-btn" onClick={onCancel}>
                <i data-lucide="x"></i>
                Close
              </button>
              <button
                type="button"
                className="tcp-btn-primary"
                disabled={pathMissing || scopeMissing || importMutation.isPending}
                onClick={() => importMutation.mutate()}
              >
                <i data-lucide="database-zap"></i>
                {importMutation.isPending ? "Importing..." : "Import to Memory"}
              </button>
            </div>
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
