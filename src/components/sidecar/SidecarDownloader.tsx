import { useState, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { RefreshCw } from "lucide-react";
import { Button, UpdateProgressUI } from "@/components/ui";

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
  latestOverallVersion?: string | null;
  updateAvailable: boolean;
  compatibilityMessage?: string | null;
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
        latestOverallVersion: null,
        updateAvailable: false,
        compatibilityMessage: null,
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

  // If showing as an update button in settings
  if (showUpdateButton && status?.installed && !status?.updateAvailable) {
    return (
      <div className="flex items-center justify-between p-4 rounded-lg bg-surface border border-border">
        <div>
          <p className="text-sm font-medium text-text">Tandem Engine</p>
          <p className="text-xs text-text-muted">Version {status.version} • Up to date</p>
        </div>
        <Button variant="ghost" size="sm" onClick={checkSidecar} className="gap-2">
          <RefreshCw className="h-3 w-3" />
          Check for Updates
        </Button>
      </div>
    );
  }

  // Determine title and description based on sidecar status
  const title =
    status?.updateAvailable && status?.version
      ? "Tandem Engine Update Available"
      : "Tandem Engine Required";

  const description = status?.compatibilityMessage
    ? status.compatibilityMessage
    : status?.updateAvailable && status?.version
      ? `Tandem Engine ${status.latestVersion} is available. You have ${status.version}.`
      : "Tandem requires the Tandem engine sidecar. This is a one-time download (~50MB).";

  const showSkip = !!status?.installed;

  return (
    <AnimatePresence mode="wait">
      <motion.div key={state} className="flex flex-col items-center justify-center p-8">
        <UpdateProgressUI
          state={state}
          progress={progress}
          title={title}
          description={description}
          version={status?.latestVersion || undefined}
          error={error}
          onAction={startDownload}
          onSkip={onComplete}
          actionLabel={status?.updateAvailable ? "Update Now" : "Download"}
          showSkip={showSkip}
        />
      </motion.div>
    </AnimatePresence>
  );
}
