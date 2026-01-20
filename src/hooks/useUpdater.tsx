import React, { createContext, useContext, useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateStatus =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "installing"
  | "installed"
  | "upToDate"
  | "error";

export interface UpdateProgress {
  downloaded: number;
  total: number;
  percent: number;
}

interface UpdaterContextType {
  status: UpdateStatus;
  updateInfo: Update | null;
  error: string | null;
  progress: UpdateProgress | null;
  isDismissed: boolean;
  checkUpdates: (silent?: boolean) => Promise<void>;
  installUpdate: () => Promise<void>;
  dismissUpdate: () => void;
}

const UpdaterContext = createContext<UpdaterContextType | null>(null);

export function UpdaterProvider({ children }: { children: React.ReactNode }) {
  const [status, setStatus] = useState<UpdateStatus>("idle");
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [isDismissed, setIsDismissed] = useState(false);

  const checkUpdates = useCallback(async (silent = false) => {
    if (!silent) {
      setStatus("checking");
    }
    setError(null);

    try {
      // On Linux, default target selection can pick the AppImage artifact even when
      // we're installed via `.deb` (`/usr/bin/tandem`). Ask the backend for a
      // target override when it can reliably detect packaging.
      const target = await invoke<string | null>("get_updater_target").catch(() => null);
      const update = await (target ? check({ target }) : check());
      if (!update) {
        if (!silent) setStatus("upToDate");
        setUpdateInfo(null);
        return;
      }

      console.log(`Update available: ${update.version}`);
      setUpdateInfo(update);
      setStatus("available");
      setIsDismissed(false); // Reset dismiss on new check finding an update
    } catch (err) {
      console.error("Update check failed:", err);
      if (!silent) {
        setStatus("error");
        setError(err instanceof Error ? err.message : "Update check failed.");
      }
    }
  }, []);

  const installUpdate = useCallback(async () => {
    if (!updateInfo) return;

    setStatus("downloading");
    setError(null);
    setProgress({ downloaded: 0, total: 0, percent: 0 });

    try {
      let downloaded = 0;
      const updateAny = updateInfo as unknown as {
        downloadAndInstall: (onEvent?: (event: any) => void) => Promise<void>;
      };

      await updateAny.downloadAndInstall((event: any) => {
        // https://v2.tauri.app/reference/javascript/updater/
        // DownloadEvent: { event: "Started"|"Progress"|"Finished", data: { contentLength?, chunkLength? } }
        const name = event?.event;
        const data = event?.data ?? {};

        if (name === "Started") {
          const total = Number(data.contentLength ?? 0);
          downloaded = 0;
          setProgress({
            downloaded: 0,
            total,
            percent: total > 0 ? 0 : 0,
          });
          return;
        }

        if (name === "Progress") {
          const chunk = Number(data.chunkLength ?? 0);
          downloaded += Number.isFinite(chunk) ? chunk : 0;
          setProgress((prev) => {
            const total = prev?.total ?? 0;
            const percent = total > 0 ? (downloaded / total) * 100 : 0;
            return {
              downloaded,
              total,
              percent: Math.max(0, Math.min(100, percent)),
            };
          });
          return;
        }

        if (name === "Finished") {
          setStatus("installing");
          return;
        }
      });
      setStatus("installed");
      await relaunch();
    } catch (err) {
      console.error("Update install failed:", err);
      setStatus("error");
      setError(err instanceof Error ? err.message : "Update install failed.");
    }
  }, [updateInfo]);

  const dismissUpdate = useCallback(() => {
    setIsDismissed(true);
  }, []);

  // Check for updates on mount
  useEffect(() => {
    const timer = setTimeout(() => {
      void checkUpdates(true);
    }, 0);
    return () => clearTimeout(timer);
  }, [checkUpdates]);

  // If dismissed, effectively hide it from the UI consumers (unless they explicitly check status)
  // But for the About page, we might want to still show it.
  // The consumer can decide how to handle "dismissed".
  // Actually, let's just expose isDismissed? No, let's keep it simple.
  // We'll expose `dismissUpdate` and let consumers filter if they want.
  // But wait, if About page uses this, dismissing the Toast shouldn't hide the "Update Available" in About.
  // So `isDismissed` should be a separate state exposed to consumers, OR consumers handle their own visibility.
  // Better: Expose `isDismissed` so the Toast can use it.

  return (
    <UpdaterContext.Provider
      value={{
        status,
        updateInfo,
        error,
        progress,
        isDismissed,
        checkUpdates,
        installUpdate,
        dismissUpdate,
      }}
    >
      {children}
    </UpdaterContext.Provider>
  );
}

export function useUpdater() {
  const context = useContext(UpdaterContext);
  if (!context) {
    throw new Error("useUpdater must be used within an UpdaterProvider");
  }
  return context;
}
