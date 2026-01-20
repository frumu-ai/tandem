import { motion, AnimatePresence } from "framer-motion";
import { Download, X, RefreshCw } from "lucide-react";
import { useUpdater } from "@/hooks/useUpdater";
import { Button } from "@/components/ui/Button";

export function UpdateToast() {
  const { status, updateInfo, installUpdate, dismissUpdate, isDismissed } = useUpdater();

  // Only show if available and not dismissed
  const show = status === "available" && updateInfo && !isDismissed;

  return (
    <AnimatePresence>
      {show && (
        <motion.div
          className="fixed bottom-6 right-6 z-50 w-80 overflow-hidden rounded-xl border border-border bg-surface shadow-2xl"
          initial={{ opacity: 0, y: 50, scale: 0.95 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 50, scale: 0.95 }}
          transition={{ type: "spring", damping: 25, stiffness: 300 }}
        >
          {/* Header */}
          <div className="flex items-center justify-between border-b border-border bg-primary/10 px-4 py-3">
            <div className="flex items-center gap-3">
              <div className="rounded-lg bg-primary/20 p-2 text-primary">
                <RefreshCw className="h-5 w-5" />
              </div>
              <div>
                <h3 className="font-semibold text-text">Update Available</h3>
                <p className="text-xs text-text-subtle">
                  Tandem v{updateInfo?.version} is ready
                </p>
              </div>
            </div>
            <button
              onClick={dismissUpdate}
              className="rounded-lg p-1 text-text-subtle hover:bg-surface hover:text-text"
            >
              <X className="h-4 w-4" />
            </button>
          </div>

          {/* Content */}
          <div className="p-4">
            <p className="text-sm text-text-muted mb-4">
              A new version of Tandem is available. Update now to get the latest features and fixes.
            </p>
            <Button onClick={installUpdate} className="w-full gap-2">
              <Download className="h-4 w-4" />
              Download & Install
            </Button>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
