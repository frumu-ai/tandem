import { AnimatePresence, motion } from "framer-motion";
import { useUpdater } from "@/hooks/useUpdater";
import { UpdateProgressUI, UpdateState } from "@/components/ui/UpdateProgressUI";

export function AppUpdateOverlay() {
  const { status, updateInfo, progress, error, installUpdate, dismissUpdate, isDismissed } =
    useUpdater();
  const updateNotes =
    typeof updateInfo?.body === "string" && updateInfo.body.trim().length > 0
      ? updateInfo.body.trim()
      : null;

  // Map updater status to UI state
  const getUiState = (): UpdateState => {
    switch (status) {
      case "checking":
        return "checking";
      case "available":
        return "idle"; // "idle" in UI terms means waiting for user action
      case "downloading":
        return "downloading";
      case "installing":
        return "installing";
      case "installed":
        return "complete";
      case "error":
        return "error";
      default:
        return "idle";
    }
  };

  // Only show if we have an update available (or are in progress) and it hasn't been dismissed
  // UNLESS we are in the middle of downloading/installing, effectively blocking the UI
  const isActive = status === "downloading" || status === "installing" || status === "installed";

  const show = (status === "available" && !isDismissed) || isActive || status === "error";

  if (!show || !updateInfo) return null;

  return (
    <AnimatePresence>
      <motion.div
        className="fixed inset-0 z-50 flex items-center justify-center bg-surface/80 backdrop-blur-sm"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
      >
        <div className="w-full max-w-2xl rounded-2xl border border-border bg-surface shadow-2xl">
          <UpdateProgressUI
            state={getUiState()}
            progress={progress || { downloaded: 0, total: 0, percent: 0 }}
            title="Tandem Update Available"
            description={`Version ${updateInfo.version} is ready to install.`}
            version={updateInfo.version}
            error={error}
            onAction={installUpdate}
            onSkip={dismissUpdate}
            actionLabel="Update Now"
            showSkip={!isActive} // Can't skip once started
          />
          {updateNotes ? (
            <div className="border-t border-border px-5 pb-5 pt-3">
              <div className="text-xs font-semibold uppercase tracking-wide text-text-subtle">
                What&apos;s New
              </div>
              <div className="mt-2 max-h-56 overflow-y-auto whitespace-pre-wrap rounded-lg border border-border bg-surface-elevated/40 p-3 text-xs text-text-subtle">
                {updateNotes}
              </div>
            </div>
          ) : null}
        </div>
      </motion.div>
    </AnimatePresence>
  );
}
