import { motion } from "framer-motion";
import { Download, CheckCircle, AlertCircle, RefreshCw, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/Button";

export type UpdateState =
  | "idle"
  | "checking"
  | "downloading"
  | "extracting"
  | "installing"
  | "complete"
  | "error";

export interface UpdateProgress {
  downloaded: number;
  total: number;
  percent: number;
  speed?: string;
}

interface UpdateProgressUIProps {
  state: UpdateState;
  progress: UpdateProgress;
  title: string;
  description: string;
  version?: string;
  error?: string | null;
  onAction: () => void;
  onSkip?: () => void;
  actionLabel: string;
  showSkip?: boolean;
}

export function UpdateProgressUI({
  state,
  progress,
  title,
  description,
  version,
  error,
  onAction,
  onSkip,
  actionLabel,
  showSkip = false,
}: UpdateProgressUIProps) {
  const normalizeVersion = (raw: string): string => {
    const trimmed = raw.trim();
    if (!trimmed) return "";
    return trimmed.replace(/^[vV]\s*/, "");
  };

  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  };

  const renderContent = () => {
    switch (state) {
      case "checking":
        return (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            className="flex flex-col items-center gap-4"
          >
            <div className="relative h-12 w-12">
              <motion.div className="absolute inset-0 rounded-full border-2 border-primary/30" />
              <motion.div
                className="absolute inset-0 rounded-full border-2 border-transparent border-t-primary"
                animate={{ rotate: 360 }}
                transition={{ duration: 1, repeat: Infinity, ease: "linear" }}
              />
            </div>
            <p className="text-sm text-primary">Checking status...</p>
          </motion.div>
        );

      case "idle":
        return (
          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            className="flex flex-col items-center gap-6"
          >
            <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-primary/10">
              <Download className="h-8 w-8 text-primary" />
            </div>

            <div className="text-center">
              <h3 className="text-lg font-semibold text-text mb-2">{title}</h3>
              <p className="text-sm text-text-muted max-w-xs">{description}</p>
              {version && (
                <p className="text-xs text-text-subtle mt-2">v{normalizeVersion(version)}</p>
              )}
            </div>

            <div className="flex gap-3">
              <Button onClick={onAction} className="gap-2">
                <Download className="h-4 w-4" />
                {actionLabel}
              </Button>
              {showSkip && onSkip && (
                <Button variant="ghost" onClick={onSkip}>
                  Skip
                </Button>
              )}
            </div>
          </motion.div>
        );

      case "downloading":
        return (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            className="flex flex-col items-center gap-6 w-full max-w-sm"
          >
            <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-primary/10">
              <motion.div
                animate={{ scale: [1, 1.1, 1] }}
                transition={{ duration: 1.5, repeat: Infinity }}
              >
                <Download className="h-8 w-8 text-primary" />
              </motion.div>
            </div>

            <div className="text-center">
              <h3 className="text-lg font-semibold text-text mb-1">Downloading</h3>
              <p className="text-sm text-text-muted">
                {formatBytes(progress.downloaded)} / {formatBytes(progress.total)}
                {progress.speed && ` • ${progress.speed}`}
              </p>
            </div>

            {/* Progress bar */}
            <div className="w-full">
              <div className="h-2 w-full rounded-full bg-surface-elevated overflow-hidden">
                <motion.div
                  className="h-full bg-gradient-to-r from-primary to-secondary"
                  initial={{ width: 0 }}
                  animate={{ width: `${progress.percent}%` }}
                  transition={{ duration: 0.3 }}
                />
              </div>
              <div className="flex justify-between mt-2 text-xs text-text-subtle">
                <span>{Math.round(progress.percent)}%</span>
              </div>
            </div>

            {/* Animated dots */}
            <div className="flex gap-1">
              {[0, 1, 2, 3, 4].map((i) => (
                <motion.div
                  key={i}
                  className="h-1.5 w-6 rounded-full bg-primary/30"
                  animate={{
                    opacity: [0.3, 1, 0.3],
                    scaleX: [1, 1.2, 1],
                  }}
                  transition={{
                    duration: 1.5,
                    repeat: Infinity,
                    delay: i * 0.2,
                  }}
                />
              ))}
            </div>
          </motion.div>
        );

      case "extracting":
        return (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            className="flex flex-col items-center gap-4"
          >
            <div className="relative h-16 w-16">
              <motion.div
                className="absolute inset-0 rounded-2xl bg-primary/10"
                animate={{ scale: [1, 1.05, 1] }}
                transition={{ duration: 1, repeat: Infinity }}
              />
              <div className="absolute inset-0 flex items-center justify-center">
                <Sparkles className="h-8 w-8 text-primary" />
              </div>
            </div>
            <div className="text-center">
              <h3 className="text-lg font-semibold text-text mb-1">Extracting</h3>
              <p className="text-sm text-text-muted">Unpacking files...</p>
            </div>
            <div className="flex gap-1">
              {[0, 1, 2, 3, 4].map((i) => (
                <motion.div
                  key={i}
                  className="h-1.5 w-6 rounded-full bg-primary/30"
                  animate={{
                    backgroundColor: [
                      "rgba(var(--color-primary-rgb), 0.3)",
                      "rgba(var(--color-primary-rgb), 1)",
                      "rgba(var(--color-primary-rgb), 0.3)",
                    ],
                  }}
                  transition={{
                    duration: 1.5,
                    repeat: Infinity,
                    delay: i * 0.2,
                  }}
                />
              ))}
            </div>
          </motion.div>
        );

      case "installing":
        return (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            className="flex flex-col items-center gap-4"
          >
            <div className="relative h-16 w-16">
              <motion.div
                className="absolute inset-0 rounded-2xl bg-primary/10"
                animate={{ rotate: [0, 90, 180, 270, 360] }}
                transition={{ duration: 2, repeat: Infinity, ease: "linear" }}
              />
              <div className="absolute inset-0 flex items-center justify-center">
                <Sparkles className="h-8 w-8 text-primary" />
              </div>
            </div>
            <div className="text-center">
              <h3 className="text-lg font-semibold text-text mb-1">Installing</h3>
              <p className="text-sm text-text-muted">Finalizing update...</p>
            </div>
          </motion.div>
        );

      case "complete":
        return (
          <motion.div
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            className="flex flex-col items-center gap-4"
          >
            <motion.div
              className="flex h-16 w-16 items-center justify-center rounded-2xl bg-success/20"
              initial={{ scale: 0 }}
              animate={{ scale: 1 }}
              transition={{ type: "spring", delay: 0.1 }}
            >
              <CheckCircle className="h-8 w-8 text-success" />
            </motion.div>
            <div className="text-center">
              <h3 className="text-lg font-semibold text-text mb-1">Ready!</h3>
              <p className="text-sm text-text-muted">Update installed successfully</p>
            </div>
          </motion.div>
        );

      case "error":
        return (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            className="flex flex-col items-center gap-4"
          >
            <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-red-500/10">
              <AlertCircle className="h-8 w-8 text-red-400" />
            </div>
            <div className="text-center">
              <h3 className="text-lg font-semibold text-white mb-1">Update Failed</h3>
              <p className="text-sm text-red-400 max-w-xs">
                {error || "An unexpected error occurred"}
              </p>
            </div>
            <Button onClick={onAction} variant="ghost" className="gap-2">
              <RefreshCw className="h-4 w-4" />
              Try Again
            </Button>
          </motion.div>
        );
    }
  };

  return <div className="flex flex-col items-center justify-center p-8">{renderContent()}</div>;
}
