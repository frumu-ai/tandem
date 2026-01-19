import { useState, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Download, CheckCircle, AlertCircle, RefreshCw, Sparkles } from "lucide-react";
import { Button } from "@/components/ui";

interface DownloadProgress {
  downloaded: number;
  total: number;
  percent: number;
  speed: string;
}

interface SidecarStatus {
  installed: boolean;
  version: string | null;
  latestVersion: string | null;
  updateAvailable: boolean;
  binaryPath: string | null;
}

type DownloadState =
  | "idle"
  | "checking"
  | "downloading"
  | "extracting"
  | "installing"
  | "complete"
  | "error";

interface SidecarDownloaderProps {
  onComplete: () => void;
  showUpdateButton?: boolean;
}

export function SidecarDownloader({
  onComplete,
  showUpdateButton = false,
}: SidecarDownloaderProps) {
  const [state, setState] = useState<DownloadState>("checking");
  const [progress, setProgress] = useState<DownloadProgress>({
    downloaded: 0,
    total: 0,
    percent: 0,
    speed: "",
  });
  const [status, setStatus] = useState<SidecarStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const checkSidecar = useCallback(async () => {
    setState("checking");
    setError(null);

    try {
      const sidecarStatus = await invoke<SidecarStatus>("check_sidecar_status");
      setStatus(sidecarStatus);

      if (sidecarStatus.installed && !sidecarStatus.updateAvailable) {
        setState("complete");
        setTimeout(onComplete, 500);
      } else {
        // Not installed or update available -> Show download UI
        setState("idle");
      }
    } catch (err) {
      console.error("Failed to check sidecar status:", err);
      // Fallback for actual errors (e.g. backend crash)
      setState("idle");
      setStatus({
        installed: false,
        version: null,
        latestVersion: null,
        updateAvailable: false,
        binaryPath: null,
      });
    }
  }, [onComplete]);

  useEffect(() => {
    // Small delay to allow fade-in transition to complete
    const timer = setTimeout(() => {
      checkSidecar();
    }, 500);
    return () => clearTimeout(timer);
  }, [checkSidecar]);

  useEffect(() => {
    // Listen for download progress events
    const unlistenProgress = listen<DownloadProgress>("sidecar-download-progress", (event) => {
      setProgress(event.payload);
    });

    const unlistenState = listen<{ state: string; error?: string }>(
      "sidecar-download-state",
      (event) => {
        const { state: newState, error: newError } = event.payload;
        setState(newState as DownloadState);
        if (newError) {
          setError(newError);
        }
        if (newState === "complete") {
          checkSidecar();
        }
      }
    );

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenState.then((fn) => fn());
    };
  }, [checkSidecar]);

  const startDownload = async () => {
    setState("downloading");
    setError(null);
    setProgress({ downloaded: 0, total: 0, percent: 0, speed: "" });

    try {
      await invoke("download_sidecar");
    } catch (err) {
      console.error("Download failed:", err);
      setState("error");
      setError(err instanceof Error ? err.message : String(err));
    }
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
            <p className="text-sm text-primary">Checking AI engine status...</p>
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
              <h3 className="text-lg font-semibold text-text mb-2">
                {status?.updateAvailable && status?.version
                  ? "OpenCode Update Available"
                  : "OpenCode AI Engine Required"}
              </h3>
              <p className="text-sm text-text-muted max-w-xs">
                {status?.updateAvailable && status?.version
                  ? `OpenCode ${status.latestVersion} is available. You have ${status.version}.`
                  : "Tandem requires the OpenCode AI engine. This is a one-time download (~50MB)."}
              </p>
              {status?.latestVersion && !status?.version && (
                <p className="text-xs text-text-subtle mt-2">OpenCode {status.latestVersion}</p>
              )}
            </div>

            <div className="flex gap-3">
              <Button onClick={startDownload} className="gap-2">
                <Download className="h-4 w-4" />
                {status?.updateAvailable ? "Update Now" : "Download"}
              </Button>
              {status?.installed && (
                <Button variant="ghost" onClick={onComplete}>
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
              <h3 className="text-lg font-semibold text-text mb-1">Downloading AI Engine</h3>
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
                <span>OpenCode AI</span>
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
            {/* Animated progress bars */}
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
              <p className="text-sm text-text-muted">Setting up AI engine...</p>
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
              <p className="text-sm text-text-muted">
                AI engine installed successfully
                {status?.version && ` (v${status.version})`}
              </p>
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
              <h3 className="text-lg font-semibold text-white mb-1">Download Failed</h3>
              <p className="text-sm text-red-400 max-w-xs">
                {error || "An unexpected error occurred"}
              </p>
            </div>
            <Button onClick={startDownload} variant="ghost" className="gap-2">
              <RefreshCw className="h-4 w-4" />
              Try Again
            </Button>
          </motion.div>
        );
    }
  };

  // If showing as an update button in settings
  if (showUpdateButton && status?.installed && !status?.updateAvailable) {
    return (
      <div className="flex items-center justify-between p-4 rounded-lg bg-surface border border-border">
        <div>
          <p className="text-sm font-medium text-text">OpenCode AI Engine</p>
          <p className="text-xs text-text-muted">Version {status.version} • Up to date</p>
        </div>
        <Button variant="ghost" size="sm" onClick={checkSidecar} className="gap-2">
          <RefreshCw className="h-3 w-3" />
          Check for Updates
        </Button>
      </div>
    );
  }

  return (
    <AnimatePresence mode="wait">
      <motion.div key={state} className="flex flex-col items-center justify-center p-8">
        {renderContent()}
      </motion.div>
    </AnimatePresence>
  );
}
