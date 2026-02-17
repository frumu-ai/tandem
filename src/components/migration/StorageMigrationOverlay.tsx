import { AnimatePresence, motion } from "framer-motion";
import { AlertTriangle, CheckCircle2, Database, Loader2, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { type StorageMigrationProgressEvent, type StorageMigrationRunResult } from "@/lib/tauri";

interface StorageMigrationOverlayProps {
  open: boolean;
  running: boolean;
  progress: StorageMigrationProgressEvent | null;
  result: StorageMigrationRunResult | null;
  onContinue: () => void;
  onRetry: () => void;
  onViewDetails: () => void;
}

const phaseLabel: Record<string, string> = {
  scanning_sources: "Scanning legacy sources",
  copying_secure_artifacts: "Copying secure artifacts",
  importing_sessions: "Importing sessions",
  rehydrating_chat_history: "Rehydrating chat history",
  validating_and_finalizing: "Validating and finalizing",
};

export function StorageMigrationOverlay({
  open,
  running,
  progress,
  result,
  onContinue,
  onRetry,
  onViewDetails,
}: StorageMigrationOverlayProps) {
  const percent = Math.max(0, Math.min(100, progress?.overall_percent ?? (running ? 5 : 100)));
  const label = phaseLabel[progress?.phase ?? ""] ?? "Preparing migration";
  const isPartial = !!result && result.status === "partial";
  const isSuccess = !!result && result.status === "success";

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-[80] flex items-center justify-center bg-black/65 backdrop-blur-md"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
        >
          <motion.div
            className="w-[min(680px,92vw)] rounded-2xl border border-primary/30 bg-surface-elevated/95 p-6 shadow-2xl"
            initial={{ scale: 0.96, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.98, opacity: 0 }}
          >
            <div className="mb-5 flex items-center gap-3">
              <div className="relative flex h-11 w-11 items-center justify-center rounded-xl border border-primary/30 bg-primary/10">
                {running ? (
                  <Loader2 className="h-5 w-5 animate-spin text-primary" />
                ) : isSuccess ? (
                  <CheckCircle2 className="h-5 w-5 text-success" />
                ) : (
                  <AlertTriangle className="h-5 w-5 text-amber-400" />
                )}
              </div>
              <div>
                <h2 className="text-lg font-semibold text-text">Migrating Your Tandem Data</h2>
                <p className="text-sm text-text-muted">
                  {running
                    ? "Please wait while we move and repair your history."
                    : isSuccess
                      ? "Migration completed successfully."
                      : "Migration completed with warnings."}
                </p>
              </div>
            </div>

            <div className="mb-5 rounded-xl border border-border bg-surface/60 p-4">
              <div className="mb-2 flex items-center justify-between text-xs text-text-subtle">
                <span>{running ? label : "Done"}</span>
                <span>{Math.round(percent)}%</span>
              </div>
              <div className="h-2 w-full overflow-hidden rounded-full bg-surface">
                <motion.div
                  className="h-full bg-gradient-to-r from-primary via-secondary to-primary"
                  initial={{ width: 0 }}
                  animate={{ width: `${percent}%` }}
                  transition={{ duration: 0.2 }}
                />
              </div>
              <div className="mt-3 grid grid-cols-2 gap-2 text-xs text-text-muted sm:grid-cols-4">
                <div className="rounded-md border border-border bg-surface/60 px-2 py-1.5">
                  Repaired sessions:{" "}
                  <span className="text-text">{progress?.sessions_repaired ?? 0}</span>
                </div>
                <div className="rounded-md border border-border bg-surface/60 px-2 py-1.5">
                  Messages: <span className="text-text">{progress?.messages_recovered ?? 0}</span>
                </div>
                <div className="rounded-md border border-border bg-surface/60 px-2 py-1.5">
                  Parts: <span className="text-text">{progress?.parts_recovered ?? 0}</span>
                </div>
                <div className="rounded-md border border-border bg-surface/60 px-2 py-1.5">
                  Errors: <span className="text-text">{progress?.error_count ?? 0}</span>
                </div>
              </div>
            </div>

            {!!result && (
              <div
                className={`mb-5 rounded-xl border px-3 py-2 text-sm ${
                  isPartial
                    ? "border-amber-500/40 bg-amber-500/10 text-amber-100"
                    : "border-success/40 bg-success/10 text-success"
                }`}
              >
                <div className="flex items-center gap-2">
                  <Database className="h-4 w-4" />
                  <span>
                    Imported {result.copied.length} artifacts, repaired {result.sessions_repaired}{" "}
                    sessions, recovered {result.messages_recovered} messages.
                  </span>
                </div>
                {isPartial && result.errors.length > 0 && (
                  <p className="mt-1 text-xs text-amber-200">
                    {result.errors.slice(0, 2).join(" | ")}
                  </p>
                )}
              </div>
            )}

            <div className="flex flex-wrap items-center justify-end gap-2">
              {!running && (
                <Button variant="ghost" onClick={onViewDetails}>
                  View Details
                </Button>
              )}
              {!running && isPartial && (
                <Button variant="secondary" onClick={onRetry}>
                  <RotateCcw className="mr-1 h-4 w-4" />
                  Retry Migration
                </Button>
              )}
              {!running && (
                <Button onClick={onContinue}>{isSuccess ? "Continue" : "Continue Anyway"}</Button>
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
