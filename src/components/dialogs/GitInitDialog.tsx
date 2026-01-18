import { motion, AnimatePresence } from "framer-motion";
import { AlertCircle, CheckCircle, GitBranch, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui";

interface GitInitDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onInitialize: () => void;
  gitInstalled: boolean;
  folderPath?: string;
}

export function GitInitDialog({ isOpen, onClose, onInitialize, gitInstalled }: GitInitDialogProps) {
  if (!gitInstalled) {
    return (
      <AnimatePresence>
        {isOpen && (
          <motion.div
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <motion.div
              className="mx-4 w-full max-w-md rounded-xl border border-border bg-surface p-6 shadow-xl"
              initial={{ scale: 0.95, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.95, opacity: 0 }}
            >
              <div className="flex items-start gap-3">
                <AlertCircle className="h-6 w-6 text-warning flex-shrink-0 mt-0.5" />
                <div className="flex-1">
                  <h2 className="text-lg font-semibold text-text mb-2">
                    Git Required for Undo Features
                  </h2>
                  <p className="text-sm text-text-muted mb-4">
                    Tandem uses Git for operation history and undo capabilities. To enable these
                    features, please install Git first.
                  </p>

                  <div className="space-y-2 mb-4">
                    <a
                      href="https://git-scm.com/download/win"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="flex items-center gap-2 text-sm text-primary hover:underline"
                    >
                      <ExternalLink className="h-4 w-4" />
                      Download Git for Windows
                    </a>
                    <p className="text-xs text-text-subtle">
                      Mac and Linux users typically have Git pre-installed.
                    </p>
                  </div>

                  <div className="flex justify-end gap-2">
                    <Button variant="ghost" onClick={onClose}>
                      Continue Without Git
                    </Button>
                  </div>
                </div>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    );
  }

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
        >
          <motion.div
            className="mx-4 w-full max-w-md rounded-xl border border-border bg-surface p-6 shadow-xl"
            initial={{ scale: 0.95, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.95, opacity: 0 }}
          >
            <div className="flex items-start gap-3">
              <GitBranch className="h-6 w-6 text-primary flex-shrink-0 mt-0.5" />
              <div className="flex-1">
                <h2 className="text-lg font-semibold text-text mb-2">Enable Version History?</h2>
                <p className="text-sm text-text-muted mb-4">
                  Initialize Git in this folder to enable:
                </p>

                <ul className="space-y-2 mb-4">
                  {["Undo AI operations", "Rewind conversations", "Review change history"].map(
                    (feature) => (
                      <li key={feature} className="flex items-start gap-2 text-sm text-text">
                        <CheckCircle className="h-4 w-4 text-success flex-shrink-0 mt-0.5" />
                        <span>{feature}</span>
                      </li>
                    )
                  )}
                </ul>

                <div className="rounded-lg bg-surface-elevated border border-border p-3 mb-4">
                  <p className="text-xs text-text-subtle">
                    <strong className="text-text">What this does:</strong> Creates a hidden{" "}
                    <code className="px-1 py-0.5 bg-surface rounded text-xs">.git</code> folder to
                    track file versions. Everything stays local on your machine.
                  </p>
                </div>

                <div className="flex justify-end gap-2">
                  <Button variant="ghost" onClick={onClose}>
                    Skip for Now
                  </Button>
                  <Button onClick={onInitialize}>Initialize Git</Button>
                </div>
              </div>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
